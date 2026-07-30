//! Version string parsing, validation, and range resolution.
//!
//! This module is the **single chokepoint** for turning untrusted version text
//! (from `runx.toml`, `.nvmrc`, `package.json`, `pyproject.toml`, ...) into a
//! concrete version that is safe to interpolate into filesystem paths and
//! download URLs.
//!
//! Two guarantees matter here:
//!
//! 1. [`validate_concrete`] rejects anything that is not a plain numeric
//!    dotted version. Version strings reach both `~/.runx/runtimes/<tool>/<version>`
//!    (which gets `remove_dir_all`'d) and `https://nodejs.org/dist/v<version>/...`,
//!    so a value like `../../../../etc` must never get that far.
//! 2. [`Req::parse`] returns `None` for ranges it cannot represent faithfully
//!    rather than guessing. Silently resolving `<20` to `20.0.0` (a version the
//!    range explicitly excludes) is worse than refusing to resolve it.

use std::fmt;

/// A parsed numeric version, e.g. `20.11.0` -> `[20, 11, 0]`.
///
/// Comparison is field-wise with missing trailing fields treated as `0`, so
/// `20.11` == `20.11.0` and `20.11` < `20.11.1`.
#[derive(Debug, Clone)]
pub struct Version(Vec<u64>);

// `PartialEq` must agree with `Ord`, which zero-pads missing components. A
// derived `PartialEq` would compare the backing `Vec` field-wise and report
// `20.11 != 20.11.0` while `cmp` reports `Equal` — an `Ord` contract violation
// that silently corrupts `max()`, `sort()`, and `BTreeMap` lookups.
//
// Note that two equal versions may still differ in `precision()`. That is
// intentional: precision is only consulted on freshly parsed range *bounds*
// (to distinguish `~20` from `~20.11`), never on candidates being compared.
impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Version {}

impl Version {
    /// Parse a dotted numeric version. Returns `None` for empty input, empty
    /// components, non-digit characters, or values that overflow `u64`.
    ///
    /// A single leading `v`/`V` is *not* stripped here; callers strip it first
    /// so that this stays a strict parser.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        for part in raw.split('.') {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            parts.push(part.parse::<u64>().ok()?);
        }
        Some(Self(parts))
    }

    /// Component at `index`, treating missing trailing components as `0`.
    fn part(&self, index: usize) -> u64 {
        self.0.get(index).copied().unwrap_or(0)
    }

    /// Render as `MAJOR.MINOR.PATCH`, padding missing components with `0`.
    pub fn to_three_parts(&self) -> String {
        format!("{}.{}.{}", self.part(0), self.part(1), self.part(2))
    }

    /// Number of components explicitly present in the source string.
    fn precision(&self) -> usize {
        self.0.len()
    }

    /// The next version that is *not* covered by a caret/tilde-style bound
    /// anchored at `self` with `keep` significant components.
    ///
    /// `bump_at(1)` on `20.11.3` yields `20.12.0`; `bump_at(0)` yields `21.0.0`.
    fn bump_at(&self, keep: usize) -> Version {
        let mut parts: Vec<u64> = (0..=keep).map(|i| self.part(i)).collect();
        parts[keep] += 1;
        Version(parts)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let len = self.precision().max(other.precision());
        for i in 0..len {
            match self.part(i).cmp(&other.part(i)) {
                std::cmp::Ordering::Equal => {}
                ordering => return ordering,
            }
        }
        std::cmp::Ordering::Equal
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered: Vec<String> = self.0.iter().map(u64::to_string).collect();
        write!(f, "{}", rendered.join("."))
    }
}

/// A single comparison against a bound version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Gte,
    Gt,
    Lte,
    Lt,
    Eq,
    Neq,
}

/// One `op version` clause, e.g. `>=3.11`.
#[derive(Debug, Clone)]
struct Clause {
    op: Op,
    bound: Version,
}

impl Clause {
    fn matches(&self, candidate: &Version) -> bool {
        let ordering = candidate.cmp(&self.bound);
        match self.op {
            Op::Gte => ordering.is_ge(),
            Op::Gt => ordering.is_gt(),
            Op::Lte => ordering.is_le(),
            Op::Lt => ordering.is_lt(),
            Op::Eq => ordering.is_eq(),
            Op::Neq => !ordering.is_eq(),
        }
    }
}

/// A version requirement: alternatives (`||`) of conjunctions (space/comma
/// separated clauses).
///
/// Supports the overlapping subset of npm `engines` and PEP 440
/// `requires-python` syntax that actually appears in real projects:
/// bare versions, `=`/`==`, `>=`, `>`, `<=`, `<`, `!=`, `^`, `~`, `~=`,
/// wildcards (`3.11.*`, `20.x`), and `||` alternation.
#[derive(Debug, Clone)]
pub struct Req {
    /// Each inner Vec is a conjunction; the outer Vec is `||` alternation.
    alternatives: Vec<Vec<Clause>>,
    /// True when the requirement was a plain exact version (`3.11.7`, `=20.1.0`)
    /// rather than something that needed range resolution.
    pub exact: bool,
}

impl Req {
    /// Parse a version requirement. Returns `None` if any part of the
    /// expression is not understood, so callers can fall back or warn instead
    /// of acting on a misparse.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }

        let mut alternatives = Vec::new();
        let mut exact = true;

        for alternative in raw.split("||") {
            let mut clauses = Vec::new();
            for term in alternative
                .split([',', ' '])
                .filter(|t| !t.trim().is_empty())
            {
                let (term_clauses, term_exact) = parse_term(term.trim())?;
                if !term_exact {
                    exact = false;
                }
                clauses.extend(term_clauses);
            }
            if clauses.is_empty() {
                return None;
            }
            alternatives.push(clauses);
        }

        if alternatives.is_empty() {
            return None;
        }
        // More than one alternative or clause is never a plain exact pin.
        if alternatives.len() > 1 || alternatives[0].len() > 1 {
            exact = false;
        }
        Some(Self {
            alternatives,
            exact,
        })
    }

    /// True when `candidate` satisfies at least one alternative.
    pub fn matches(&self, candidate: &Version) -> bool {
        self.alternatives
            .iter()
            .any(|clauses| clauses.iter().all(|clause| clause.matches(candidate)))
    }

    /// The lowest version this requirement admits, if one can be determined.
    ///
    /// Returns `None` when the requirement has no lower bound (e.g. `<20`),
    /// because there is no defensible "minimum" to pick — every answer would be
    /// a guess. Callers should surface that instead of inventing a version.
    pub fn minimum(&self) -> Option<Version> {
        let mut best: Option<Version> = None;
        for clauses in &self.alternatives {
            let candidate = minimum_of_conjunction(clauses)?;
            if best.as_ref().is_none_or(|current| candidate < *current) {
                best = Some(candidate);
            }
        }
        best
    }

    /// The highest version in `available` that satisfies this requirement.
    ///
    /// This is how nvm/volta/mise behave for ranges, and is what
    /// [`crate::detect`] uses once a real release list is available.
    pub fn best_match<'a, I>(&self, available: I) -> Option<Version>
    where
        I: IntoIterator<Item = &'a Version>,
    {
        available
            .into_iter()
            .filter(|candidate| self.matches(candidate))
            .max()
            .cloned()
    }
}

/// Lowest version satisfying every clause in a conjunction.
fn minimum_of_conjunction(clauses: &[Clause]) -> Option<Version> {
    // Start from the strongest explicit lower bound.
    let mut lower: Option<Version> = None;
    for clause in clauses {
        let candidate = match clause.op {
            Op::Gte | Op::Eq => clause.bound.clone(),
            // `>1.2.3` excludes 1.2.3 itself; the next representable release at
            // the bound's precision is the smallest safe answer.
            Op::Gt => clause
                .bound
                .bump_at(clause.bound.precision().saturating_sub(1)),
            // Upper bounds and exclusions provide no lower bound.
            Op::Lte | Op::Lt | Op::Neq => continue,
        };
        if lower.as_ref().is_none_or(|current| candidate > *current) {
            lower = Some(candidate);
        }
    }

    let lower = lower?;
    // The computed floor must actually satisfy the whole conjunction; if an
    // upper bound or `!=` excludes it we cannot resolve without a release list.
    if clauses.iter().all(|clause| clause.matches(&lower)) {
        Some(lower)
    } else {
        None
    }
}

/// Parse one whitespace-free term into clauses.
///
/// Returns the clauses plus whether the term was an exact pin.
fn parse_term(term: &str) -> Option<(Vec<Clause>, bool)> {
    // Wildcards: `*`, `x`, and `latest` mean "anything".
    if matches!(term, "*" | "x" | "X" | "latest") {
        return Some((
            vec![Clause {
                op: Op::Gte,
                bound: Version(vec![0]),
            }],
            false,
        ));
    }

    // Ordered longest-first so `>=` is not read as `>`.
    const OPERATORS: &[(&str, Op)] = &[
        (">=", Op::Gte),
        ("<=", Op::Lte),
        ("==", Op::Eq),
        ("!=", Op::Neq),
        ("~=", Op::Gte), // PEP 440 compatible-release; refined below.
        (">", Op::Gt),
        ("<", Op::Lt),
        ("=", Op::Eq),
        ("^", Op::Gte), // npm caret; refined below.
        ("~", Op::Gte), // npm tilde; refined below.
    ];

    for &(token, op) in OPERATORS {
        let Some(rest) = term.strip_prefix(token) else {
            continue;
        };
        let rest = rest.trim().trim_start_matches(['v', 'V']);
        let bound = Version::parse(rest)?;

        return match token {
            // `^20` => >=20.0.0, <21.0.0   (`^0.2.3` pins the minor per npm)
            "^" => {
                let pivot = if bound.part(0) == 0 { 1 } else { 0 };
                Some((upper_bounded(bound, pivot), false))
            }
            // `~20.11` => >=20.11.0, <20.12.0; `~20` => >=20.0.0, <21.0.0
            "~" => {
                let pivot = if bound.precision() >= 2 { 1 } else { 0 };
                Some((upper_bounded(bound, pivot), false))
            }
            // PEP 440 `~=3.11` => >=3.11, <4.0; `~=3.11.7` => >=3.11.7, <3.12.0
            "~=" => {
                let pivot = bound.precision().saturating_sub(2);
                Some((upper_bounded(bound, pivot), false))
            }
            "==" | "=" => Some((vec![Clause { op, bound }], true)),
            _ => Some((vec![Clause { op, bound }], false)),
        };
    }

    // Trailing wildcard: `3.11.*` / `20.x` => >=3.11.0, <3.12.0
    if let Some(prefix) = term
        .strip_suffix(".*")
        .or_else(|| term.strip_suffix(".x"))
        .or_else(|| term.strip_suffix(".X"))
    {
        let bound = Version::parse(prefix.trim_start_matches(['v', 'V']))?;
        let pivot = bound.precision().saturating_sub(1);
        return Some((upper_bounded(bound, pivot), false));
    }

    // Bare version: an exact pin.
    let bound = Version::parse(term.trim_start_matches(['v', 'V']))?;
    Some((vec![Clause { op: Op::Eq, bound }], true))
}

/// Build `>=bound, <bound.bump_at(pivot)`.
fn upper_bounded(bound: Version, pivot: usize) -> Vec<Clause> {
    let upper = bound.bump_at(pivot);
    vec![
        Clause { op: Op::Gte, bound },
        Clause {
            op: Op::Lt,
            bound: upper,
        },
    ]
}

/// Reject any version string that is not a plain numeric dotted version.
///
/// This is the security boundary: the returned string is interpolated into
/// cache paths (which are recursively deleted on reinstall) and into download
/// URLs. Restricting the alphabet to digits and `.` makes path traversal
/// (`../`), absolute paths, UNC paths, URL injection (`?`, `#`, `@`, `//`) and
/// shell metacharacters unrepresentable rather than merely filtered.
pub fn validate_concrete(tool: &str, version: &str) -> Result<(), String> {
    let invalid = || {
        format!(
            "Invalid version `{version}` for runtime `{tool}`.\n\
             Expected MAJOR.MINOR.PATCH with numeric parts (e.g. 20.11.0).\n\
             Hint: run `runx init` to see example versions."
        )
    };

    let parsed = Version::parse(version).ok_or_else(invalid)?;

    if parsed.precision() != 3 {
        return Err(format!(
            "Incomplete version `{version}` for runtime `{tool}`.\n\
             Expected all three of MAJOR.MINOR.PATCH (e.g. 20.11.0)."
        ));
    }

    // Require the input to be byte-identical to its canonical rendering.
    //
    // `Version::parse` tolerates surrounding whitespace, but the *caller* keeps
    // using the original string for the cache path and download URL. Without
    // this check, `"20.11.0\n"` would validate while a newline reached the
    // filesystem and the URL. Comparing against the canonical form keeps
    // "what was validated" and "what gets used" the same bytes, and also
    // collapses aliases like `020.11.0` that would otherwise create a second
    // cache directory for one release.
    if version != parsed.to_three_parts() {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(raw: &str) -> Version {
        Version::parse(raw).expect("valid version")
    }

    fn minimum(raw: &str) -> Option<String> {
        Req::parse(raw)?.minimum().map(|m| m.to_three_parts())
    }

    // ── Rejection of unsafe / unparseable input ───────────────────────────────

    #[test]
    fn rejects_path_traversal_payloads() {
        for payload in [
            "../../../../tmp/pwned",
            "..",
            "../",
            "/etc/passwd",
            "..\\..\\windows",
            "20.11.0/../../etc",
            "C:\\windows",
            "\\\\server\\share",
        ] {
            assert!(
                Version::parse(payload).is_none(),
                "{payload} must not parse as a version"
            );
            assert!(
                validate_concrete("node", payload).is_err(),
                "{payload} must be rejected by validate_concrete"
            );
        }
    }

    #[test]
    fn rejects_url_and_shell_injection_payloads() {
        for payload in [
            "20.11.0?x=1",
            "20.11.0#frag",
            "evil.com/x",
            "20.11.0 && rm -rf /",
            "$(whoami)",
            "20.11.0\n",
            "lts/iron",
            "latest",
            "",
            "  ",
        ] {
            assert!(
                validate_concrete("node", payload).is_err(),
                "{payload:?} must be rejected"
            );
        }
    }

    #[test]
    fn accepts_plain_three_part_versions() {
        for good in ["20.11.0", "3.11.7", "0.0.1", "1.23.4"] {
            assert!(
                validate_concrete("node", good).is_ok(),
                "{good} should pass"
            );
        }
    }

    #[test]
    fn requires_all_three_components() {
        assert!(validate_concrete("node", "20").is_err());
        assert!(validate_concrete("node", "20.11").is_err());
    }

    #[test]
    fn rejects_leading_v_at_the_boundary() {
        // Callers strip `v` during detection; the boundary itself stays strict.
        assert!(validate_concrete("node", "v20.11.0").is_err());
    }

    // ── Range resolution correctness ──────────────────────────────────────────

    #[test]
    fn resolves_lower_bounded_ranges_to_their_floor() {
        assert_eq!(minimum(">=3.11").as_deref(), Some("3.11.0"));
        assert_eq!(minimum(">=20.11.0").as_deref(), Some("20.11.0"));
        assert_eq!(minimum("^20").as_deref(), Some("20.0.0"));
        assert_eq!(minimum("~20.11").as_deref(), Some("20.11.0"));
        assert_eq!(minimum("~=3.11").as_deref(), Some("3.11.0"));
        assert_eq!(minimum("3.11.*").as_deref(), Some("3.11.0"));
        assert_eq!(minimum("20.x").as_deref(), Some("20.0.0"));
    }

    #[test]
    fn exact_pins_are_marked_exact() {
        assert!(Req::parse("20.11.0").expect("parse").exact);
        assert!(Req::parse("==3.11.7").expect("parse").exact);
        assert!(Req::parse("=20.11.0").expect("parse").exact);
        assert!(!Req::parse(">=3.11").expect("parse").exact);
        assert!(!Req::parse("^20").expect("parse").exact);
    }

    /// Regression: the old resolver mapped `<20` to `20.0.0`, a version the
    /// range explicitly excludes.
    #[test]
    fn upper_bound_only_ranges_have_no_minimum() {
        assert_eq!(minimum("<20"), None);
        assert_eq!(minimum("<=20"), None);
        let req = Req::parse("<20").expect("parse");
        assert!(!req.matches(&v("20.0.0")), "20.0.0 must not satisfy <20");
        assert!(req.matches(&v("19.9.0")));
    }

    /// Regression: the old resolver turned `!=3.11` into `3.11.0` — the one
    /// version the constraint forbids.
    #[test]
    fn not_equal_never_resolves_to_the_excluded_version() {
        let req = Req::parse("!=3.11").expect("parse");
        assert!(!req.matches(&v("3.11.0")));
        assert!(req.matches(&v("3.12.0")));
        assert_eq!(minimum("!=3.11"), None);
    }

    /// Regression: the old resolver produced the literal string
    /// `"18 || >=20.0.0"` and used it as a directory name and URL segment.
    #[test]
    fn alternation_picks_the_lowest_alternative() {
        let req = Req::parse("18 || >=20").expect("parse");
        assert!(req.matches(&v("18.0.0")));
        assert!(req.matches(&v("22.1.0")));
        assert!(!req.matches(&v("19.0.0")));
        assert_eq!(minimum("18 || >=20").as_deref(), Some("18.0.0"));
    }

    #[test]
    fn caret_and_tilde_upper_bounds_are_enforced() {
        let caret = Req::parse("^20.11.0").expect("parse");
        assert!(caret.matches(&v("20.99.0")));
        assert!(!caret.matches(&v("21.0.0")));
        assert!(!caret.matches(&v("20.10.0")));

        let tilde = Req::parse("~20.11").expect("parse");
        assert!(tilde.matches(&v("20.11.9")));
        assert!(!tilde.matches(&v("20.12.0")));
    }

    /// npm treats `^0.x.y` as pinning the minor component.
    #[test]
    fn caret_below_one_pins_the_minor() {
        let req = Req::parse("^0.2.3").expect("parse");
        assert!(req.matches(&v("0.2.9")));
        assert!(!req.matches(&v("0.3.0")));
    }

    #[test]
    fn pep440_compatible_release_bounds_match_the_spec() {
        // ~=3.11 allows 3.x >= 3.11 but not 4.0
        let two = Req::parse("~=3.11").expect("parse");
        assert!(two.matches(&v("3.12.0")));
        assert!(!two.matches(&v("4.0.0")));

        // ~=3.11.7 allows 3.11.x >= 3.11.7 but not 3.12
        let three = Req::parse("~=3.11.7").expect("parse");
        assert!(three.matches(&v("3.11.9")));
        assert!(!three.matches(&v("3.12.0")));
        assert!(!three.matches(&v("3.11.6")));
    }

    #[test]
    fn compound_ranges_are_intersected() {
        let req = Req::parse(">=3.9, <3.12").expect("parse");
        assert!(req.matches(&v("3.11.7")));
        assert!(!req.matches(&v("3.12.0")));
        assert!(!req.matches(&v("3.8.0")));
        assert_eq!(minimum(">=3.9, <3.12").as_deref(), Some("3.9.0"));
    }

    #[test]
    fn space_separated_compound_ranges_parse() {
        let req = Req::parse(">=14 <17").expect("parse");
        assert!(req.matches(&v("16.0.0")));
        assert!(!req.matches(&v("17.0.0")));
    }

    #[test]
    fn greater_than_excludes_the_bound_itself() {
        let req = Req::parse(">20.11.0").expect("parse");
        assert!(!req.matches(&v("20.11.0")));
        assert!(req.matches(&v("20.11.1")));
        assert_eq!(minimum(">20.11.0").as_deref(), Some("20.11.1"));
    }

    #[test]
    fn unparseable_requirements_return_none_instead_of_guessing() {
        for bad in ["lts/iron", "nightly", "", "  ", ">=", "^", "abc", ">=3.x.y"] {
            assert!(Req::parse(bad).is_none(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn wildcards_match_anything() {
        for wildcard in ["*", "x", "latest"] {
            let req = Req::parse(wildcard).expect("parse");
            assert!(req.matches(&v("0.0.1")));
            assert!(req.matches(&v("99.0.0")));
        }
    }

    // ── best_match: latest-satisfying resolution ──────────────────────────────

    #[test]
    fn best_match_picks_the_newest_satisfying_release() {
        let available: Vec<Version> = ["3.10.13", "3.11.0", "3.11.7", "3.12.1", "3.13.0"]
            .iter()
            .map(|s| v(s))
            .collect();

        let req = Req::parse(">=3.11").expect("parse");
        assert_eq!(
            req.best_match(&available).map(|m| m.to_three_parts()),
            Some("3.13.0".to_string()),
            ">=3.11 should resolve to the newest available, like nvm/mise"
        );

        let bounded = Req::parse("~=3.11").expect("parse");
        assert_eq!(
            bounded.best_match(&available).map(|m| m.to_three_parts()),
            Some("3.13.0".to_string())
        );

        let pinned = Req::parse("3.11.7").expect("parse");
        assert_eq!(
            pinned.best_match(&available).map(|m| m.to_three_parts()),
            Some("3.11.7".to_string())
        );
    }

    #[test]
    fn best_match_returns_none_when_nothing_satisfies() {
        let available = vec![v("18.0.0"), v("20.0.0")];
        let req = Req::parse(">=22").expect("parse");
        assert_eq!(req.best_match(&available), None);
    }

    #[test]
    fn best_match_respects_upper_bounds() {
        let available = vec![v("18.0.0"), v("19.5.0"), v("20.0.0"), v("21.0.0")];
        let req = Req::parse("<20").expect("parse");
        assert_eq!(
            req.best_match(&available).map(|m| m.to_three_parts()),
            Some("19.5.0".to_string()),
            "an upper-bounded range resolves fine against a real release list"
        );
    }

    // ── Ordering ──────────────────────────────────────────────────────────────

    #[test]
    fn ordering_treats_missing_components_as_zero() {
        assert_eq!(v("20.11"), v("20.11.0"));
        assert!(v("20.11") < v("20.11.1"));
        assert!(v("20.9.0") < v("20.11.0"), "numeric, not lexicographic");
        assert!(v("3.9.0") < v("3.10.0"));
    }
}
