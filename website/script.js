(function () {
  "use strict";

  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var $ = function (sel, root) { return (root || document).querySelector(sel); };
  var $$ = function (sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); };

  /* ---------- theme ---------- */
  var root = document.documentElement;
  var themeBtn = $("#theme-toggle");
  function applyTheme(theme) {
    root.dataset.theme = theme;
    if (themeBtn) {
      themeBtn.setAttribute("aria-label", theme === "dark" ? "Switch to light theme" : "Switch to dark theme");
      themeBtn.setAttribute("aria-pressed", theme === "dark" ? "true" : "false");
    }
    $$('meta[name="theme-color"]').forEach(function (meta) {
      meta.setAttribute("content", theme === "dark" ? "#0a0c0b" : "#fcfcfa");
    });
  }
  applyTheme(root.dataset.theme || (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"));
  if (themeBtn) {
    themeBtn.addEventListener("click", function () {
      var next = root.dataset.theme === "dark" ? "light" : "dark";
      try { localStorage.setItem("runx-theme", next); } catch (e) {}
      applyTheme(next);
    });
  }

  /* ---------- header / mobile nav ---------- */
  var header = $(".site-header");
  var nav = $("#nav");
  var menuBtn = $("#menu-btn");
  function closeNav() {
    if (!nav || !menuBtn) return;
    nav.classList.remove("open");
    menuBtn.setAttribute("aria-expanded", "false");
    menuBtn.setAttribute("aria-label", "Open menu");
    document.body.classList.remove("nav-open");
  }
  if (menuBtn && nav) {
    menuBtn.addEventListener("click", function () {
      var open = nav.classList.toggle("open");
      menuBtn.setAttribute("aria-expanded", open ? "true" : "false");
      menuBtn.setAttribute("aria-label", open ? "Close menu" : "Open menu");
      document.body.classList.toggle("nav-open", open);
      if (open) {
        var first = nav.querySelector("a");
        if (first) first.focus();
      }
    });
    $$("a", nav).forEach(function (a) { a.addEventListener("click", closeNav); });
    window.addEventListener("keydown", function (e) {
      if (e.key === "Escape") { closeNav(); menuBtn.focus(); }
    });
  }
  function onScroll() {
    if (header) header.classList.toggle("scrolled", window.scrollY > 8);
  }
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  /* ---------- scroll reveal ---------- */
  var revealEls = $$(".reveal");
  if (!("IntersectionObserver" in window) || reduced) {
    revealEls.forEach(function (el) { el.classList.add("in"); });
  } else {
    var revealIO = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (!entry.isIntersecting) return;
        entry.target.classList.add("in");
        revealIO.unobserve(entry.target);
      });
    }, { threshold: 0.12, rootMargin: "0px 0px -40px 0px" });
    revealEls.forEach(function (el) { revealIO.observe(el); });
  }

  /* ---------- pipeline ---------- */
  var steps = $$("[data-pipeline] .pipe-step");
  if (steps.length) {
    if (reduced || !("IntersectionObserver" in window)) {
      steps.forEach(function (s) { s.classList.add("lit"); });
    } else {
      var pipeIO = new IntersectionObserver(function (entries) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) return;
          steps.forEach(function (s, i) {
            setTimeout(function () { s.classList.add("lit"); }, i * 110);
          });
          pipeIO.disconnect();
        });
      }, { threshold: 0.25 });
      pipeIO.observe(steps[0].parentElement);
    }
  }

  if (!/Mac|iPhone|iPad/.test(navigator.platform || "")) {
    $$(".palette-key").forEach(function (el) { el.textContent = "Ctrl K"; });
  }

  /* ---------- active section ---------- */
  var navLinks = $$("#nav a[href^='#']");
  var sections = navLinks.map(function (a) { return document.getElementById(a.getAttribute("href").slice(1)); }).filter(Boolean);
  if ("IntersectionObserver" in window && sections.length) {
    var activeIO = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (!entry.isIntersecting) return;
        navLinks.forEach(function (a) {
          var on = a.getAttribute("href") === "#" + entry.target.id;
          if (on) a.setAttribute("aria-current", "true");
          else a.removeAttribute("aria-current");
        });
      });
    }, { rootMargin: "-45% 0px -50% 0px", threshold: 0 });
    sections.forEach(function (s) { activeIO.observe(s); });
  }

  /* ---------- focusable scroll regions ---------- */
  function markScrollable() {
    $$(".cmd-line code, .how-code pre, .cli-body pre, .table-scroll, .bench-table-wrap").forEach(function (el) {
      if (el.scrollWidth > el.clientWidth + 1) el.setAttribute("tabindex", "0");
      else el.removeAttribute("tabindex");
    });
  }
  markScrollable();
  window.addEventListener("resize", markScrollable, { passive: true });

  /* ---------- copy ---------- */
  function legacyCopy(text) {
    return new Promise(function (resolve, reject) {
      var ta = document.createElement("textarea");
      ta.value = text;
      ta.setAttribute("readonly", "");
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); resolve(); } catch (e) { reject(e); }
      document.body.removeChild(ta);
    });
  }
  function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      return navigator.clipboard.writeText(text).catch(function () { return legacyCopy(text); });
    }
    return legacyCopy(text);
  }
  $$(".copy-btn, .copy-install").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var text = btn.getAttribute("data-copy");
      if (!text) return;
      copyText(text).then(function () {
        var label = btn.querySelector(".copy-label") || btn;
        var original = label.textContent;
        label.textContent = "Copied ✓";
        btn.classList.add("copied");
        clearTimeout(btn._copyTimer);
        btn._copyTimer = setTimeout(function () {
          label.textContent = original;
          btn.classList.remove("copied");
        }, 1800);
      }).catch(function () {
        var label = btn.querySelector(".copy-label") || btn;
        label.textContent = "Copy failed";
        setTimeout(function () { label.textContent = "Copy"; }, 1800);
      });
    });
  });

  /* ---------- terminal demo ---------- */
  var demo = {
    node: {
      file: ".nvmrc",
      lines: [
        ["dim", "→ reading project files"],
        ["ok", "✓ detected node 20.11.0 (.nvmrc)"],
        ["dim", "↓ downloading node 20.11.0 (linux-x64)"],
        ["ok", "✓ sha-256 verified"],
        ["dim", "→ child PATH isolated · shell unchanged"],
        ["", "> npm run dev"],
        ["dim", "  VITE v6.2.3  ready in 142 ms"],
        ["ok", "✓ http://localhost:3000"]
      ]
    },
    python: {
      file: ".python-version",
      lines: [
        ["dim", "→ reading project files"],
        ["ok", "✓ detected python 3.13 (.python-version)"],
        ["dim", "Resolved python `3.13` to 3.13.15"],
        ["dim", "↓ downloading python-build-standalone (linux-x64)"],
        ["ok", "✓ sha-256 verified"],
        ["dim", "→ child PATH isolated · shell unchanged"],
        ["", "> uvicorn app:app --reload"],
        ["ok", "✓ Uvicorn running on http://127.0.0.1:8000"]
      ]
    },
    go: {
      file: "go.mod",
      lines: [
        ["dim", "→ reading project files"],
        ["ok", "✓ detected go 1.23 (go.mod)"],
        ["dim", "↓ downloading go1.23.4 (linux-amd64)"],
        ["ok", "✓ sha-256 verified"],
        ["dim", "→ child PATH isolated · shell unchanged"],
        ["", "> go run ./cmd/server"],
        ["ok", "✓ listening on :8080"]
      ]
    },
    bun: {
      file: "bun.lock",
      lines: [
        ["dim", "→ reading project files"],
        ["ok", "✓ detected bun 1.3.14 (bun.lock)"],
        ["dim", "↓ downloading bun-linux-x64.zip"],
        ["ok", "✓ sha-256 verified"],
        ["dim", "→ child PATH isolated · shell unchanged"],
        ["", "> bun run dev"],
        ["ok", "✓ ready in 212 ms"],
        ["ok", "✓ http://localhost:3000"]
      ]
    },
    deno: {
      file: ".dvmrc",
      lines: [
        ["dim", "→ reading project files"],
        ["ok", "✓ detected deno 2.5.0 (.dvmrc)"],
        ["dim", "↓ downloading deno-x86_64-unknown-linux-gnu.zip"],
        ["ok", "✓ sha-256 verified"],
        ["dim", "→ child PATH isolated · shell unchanged"],
        ["", "> deno task dev"],
        ["dim", "  Task dev deno run --allow-net main.ts"],
        ["ok", "✓ Listening on http://0.0.0.0:8000"]
      ]
    }
  };
  var out = $("#term-output");
  var demoBox = $("#demo");
  var runToken = 0;
  var currentRt = "node";

  function lineEl(cls, text) {
    var span = document.createElement("span");
    span.className = "ln" + (cls ? " " + cls : "");
    span.textContent = text;
    return span;
  }
  function renderDemo(rt, animate) {
    if (!out) return;
    var data = demo[rt] || demo.node;
    var token = ++runToken;
    out.textContent = "";
    var cmd = lineEl("cmd", "");
    var prompt = document.createElement("span");
    prompt.className = "prompt";
    prompt.textContent = "$ ";
    cmd.appendChild(prompt);
    var cmdText = document.createElement("span");
    cmd.appendChild(cmdText);
    out.appendChild(cmd);

    if (!animate || reduced) {
      cmdText.textContent = "runx dev";
      data.lines.forEach(function (l) { out.appendChild(lineEl(l[0], l[1])); });
      return;
    }
    cmd.classList.add("typing");
    var text = "runx dev";
    var i = 0;
    function type() {
      if (token !== runToken) return;
      cmdText.textContent = text.slice(0, ++i);
      if (i < text.length) { setTimeout(type, 52 + Math.random() * 40); return; }
      cmd.classList.remove("typing");
      var n = 0;
      function step() {
        if (token !== runToken) return;
        if (n >= data.lines.length) return;
        var l = data.lines[n++];
        out.appendChild(lineEl(l[0], l[1]));
        setTimeout(step, 260 + Math.random() * 190);
      }
      setTimeout(step, 280);
    }
    setTimeout(type, 220);
  }

  if (out && demoBox) {
    var started = false;
    var startDemo = function () {
      if (started) return;
      started = true;
      renderDemo(currentRt, true);
    };
    if (reduced || !("IntersectionObserver" in window)) {
      startDemo();
    } else {
      var demoIO = new IntersectionObserver(function (entries) {
        entries.forEach(function (e) {
          if (e.isIntersecting) { startDemo(); demoIO.disconnect(); }
        });
      }, { threshold: 0.3 });
      demoIO.observe(demoBox);
    }

    $$(".rt-btn").forEach(function (btn) {
      btn.addEventListener("click", function () {
        currentRt = btn.getAttribute("data-rt");
        $$(".rt-btn").forEach(function (b) {
          b.setAttribute("aria-pressed", b === btn ? "true" : "false");
        });
        started = true;
        renderDemo(currentRt, true);
      });
    });
    var replay = $("#demo-replay");
    if (replay) replay.addEventListener("click", function () { started = true; renderDemo(currentRt, true); });
  }

  /* ---------- install tabs ---------- */
  var tabs = $$(".os-tabs [role='tab']");
  function selectTab(tab) {
    tabs.forEach(function (t) {
      var on = t === tab;
      t.setAttribute("aria-selected", on ? "true" : "false");
      t.tabIndex = on ? 0 : -1;
      var panel = document.getElementById(t.getAttribute("aria-controls"));
      if (panel) panel.hidden = !on;
    });
  }
  if (tabs.length) {
    var initialTab = tabs.filter(function (t) { return t.getAttribute("aria-selected") === "true"; })[0] || tabs[0];
    selectTab(initialTab);
    tabs.forEach(function (tab, index) {
      tab.addEventListener("click", function () { selectTab(tab); });
      tab.addEventListener("keydown", function (e) {
        var next = null;
        if (e.key === "ArrowRight") next = tabs[(index + 1) % tabs.length];
        if (e.key === "ArrowLeft") next = tabs[(index - 1 + tabs.length) % tabs.length];
        if (e.key === "Home") next = tabs[0];
        if (e.key === "End") next = tabs[tabs.length - 1];
        if (next) { e.preventDefault(); selectTab(next); next.focus(); }
      });
    });
  }

  /* ---------- detection picker ---------- */
  var detection = {
    nvmrc: {
      output: '<span class="dim">No runx.toml found — detected from project files:</span>\n  <span class="ok">node 20.11.0</span> (from .nvmrc)\n<span class="dim">Using cached node 20.11.0 at ~/.runx/runtimes/node/20.11.0</span>\nRunning `npm run dev`',
      note: "Plain text, leading <code>v</code> stripped. Highest-priority Node source."
    },
    engines: {
      output: '<span class="dim">No runx.toml found — detected from project files:</span>\n  <span class="ok">node >=20</span> (from package.json (engines.node))\n<span class="dim">Resolved node `>=20` to the newest matching release</span>\nRunning `npm run dev`',
      note: "The range is resolved against the vendor index; the concrete version is reported before running."
    },
    pyversion: {
      output: '<span class="dim">No runx.toml found — detected from project files:</span>\n  <span class="ok">python 3.12</span> (from .python-version)\n<span class="dim">Resolved python `3.12` to the newest 3.12.x</span>\nRunning `python app.py`',
      note: "A bare minor is an X-range: newest 3.12.x. Exact patches avoid list lookups."
    },
    pyproject: {
      output: '<span class="dim">No runx.toml found — detected from project files:</span>\n  <span class="ok">python >=3.11</span> (from pyproject.toml (requires-python))\n<span class="dim">Resolved python `>=3.11` to the newest stable release</span>\nRunning `python -m app`',
      note: "Read from <code>[project].requires-python</code>. <code>Pipfile</code> is the fallback."
    },
    gomod: {
      output: '<span class="dim">No runx.toml found — detected from project files:</span>\n  <span class="ok">go 1.23</span> (from go.mod)\n<span class="dim">Resolved go `1.23` to the newest 1.23.x</span>\nRunning `go run ./cmd/server`',
      note: "The <code>go</code> directive is used; the <code>toolchain</code> directive is ignored."
    },
    bunversion: {
      output: '<span class="dim">No runx.toml found — detected from project files:</span>\n  <span class="ok">bun 1.3.14</span> (from .bun-version)\n<span class="dim">Using cached bun 1.3.14</span>\nRunning `bun run dev`',
      note: "Bun markers include <code>.bun-version</code>, <code>engines.bun</code>, <code>packageManager</code>, <code>bun.lock</code> and <code>bunfig.toml</code>."
    },
    dvmrc: {
      output: '<span class="dim">No runx.toml found — detected from project files:</span>\n  <span class="ok">deno 2.5.0</span> (from .dvmrc)\n<span class="dim">Using cached deno 2.5.0</span>\nRunning `deno task dev`',
      note: "Deno is detected from <code>.dvmrc</code> / <code>.deno-version</code>, or an explicit entry in <code>runx.toml</code>."
    },
    scripts: {
      output: '<span class="dim">No runx.toml found — detected from project files:</span>\n  <span class="ok">node 20.11.0</span> (from .nvmrc)\n\n<span class="dim">Available commands from package.json:</span>\n  dev    npm run dev\n  build  npm run build\n  test   npm test\n\nRunning `npm run dev`',
      note: "Every <code>package.json</code> script becomes a command. No other commands are guessed."
    }
  };
  var detectOut = $("#detect-output");
  var detectNote = $("#detect-note");
  if (detectOut) {
    var fileButtons = $$(".file-btn");
    fileButtons.forEach(function (btn) {
      btn.addEventListener("click", function () {
        var item = detection[btn.getAttribute("data-detect")];
        if (!item) return;
        fileButtons.forEach(function (b) { b.setAttribute("aria-pressed", b === btn ? "true" : "false"); });
        detectOut.innerHTML = item.output;
        if (detectNote) {
          detectNote.innerHTML = item.note + " Explicit <code>runx.toml</code> always wins; detection never writes to disk.";
        }
        if (window.matchMedia("(max-width: 900px)").matches) {
          var result = document.getElementById("detect-result");
          if (result) result.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "center" });
        }
      });
    });
  }

  /* ---------- CLI filter ---------- */
  var cliSearch = $("#cli-search");
  if (cliSearch) {
    var cliItems = $$("#cli-list .cli-item");
    var cliCount = $("#cli-count");
    var filterCli = function () {
      var q = cliSearch.value.trim().toLowerCase();
      var shown = 0;
      cliItems.forEach(function (item) {
        var match = !q || item.textContent.toLowerCase().indexOf(q) !== -1;
        item.hidden = !match;
        if (match) shown++;
      });
      if (cliCount) cliCount.textContent = shown + " of " + cliItems.length + " entries";
    };
    cliSearch.addEventListener("input", filterCli);
    filterCli();
  }

  /* ---------- command palette ---------- */
  var palette = $("#palette");
  var paletteInput = $("#palette-input");
  var paletteList = $("#palette-list");
  var paletteOpen = $("#palette-open");
  var paletteEntries = [
    { label: "Overview / top", href: "#top", group: "Section" },
    { label: "How it works", href: "#how", group: "Section" },
    { label: "Quick start / install", href: "#install", group: "Section" },
    { label: "Zero-config detection", href: "#detect", group: "Section" },
    { label: "Security & verification", href: "#security", group: "Section" },
    { label: "Benchmarks", href: "#bench", group: "Section" },
    { label: "Compare with mise, asdf, nvm", href: "#compare", group: "Section" },
    { label: "CLI reference", href: "#cli", group: "Section" },
    { label: "Roadmap & changelog", href: "#roadmap", group: "Section" },
    { label: "FAQ", href: "#faq", group: "Section" },
    { label: "Contributing", href: "#contribute", group: "Section" },
    { label: "GitHub repository", href: "https://github.com/aryankahar31/runx", group: "Link" },
    { label: "Latest release", href: "https://github.com/aryankahar31/runx/releases/latest", group: "Link" },
    { label: "Changelog", href: "https://github.com/aryankahar31/runx/blob/main/CHANGELOG.md", group: "Link" },
    { label: "Documentation", href: "https://github.com/aryankahar31/runx/tree/main/docs", group: "Link" },
    { label: "Security model", href: "https://github.com/aryankahar31/runx/blob/main/docs/security.md", group: "Link" },
    { label: "Benchmark methodology", href: "https://github.com/aryankahar31/runx/blob/main/benchmarks/README.md", group: "Link" },
    { label: "Comparison matrix", href: "https://github.com/aryankahar31/runx/blob/main/docs/comparison.md", group: "Link" }
  ];
  var filtered = paletteEntries.slice();
  var selected = 0;

  function renderPalette() {
    if (!paletteList) return;
    var q = (paletteInput.value || "").trim().toLowerCase();
    filtered = paletteEntries.filter(function (e) { return !q || e.label.toLowerCase().indexOf(q) !== -1; });
    selected = 0;
    paletteList.textContent = "";
    if (!filtered.length) {
      var empty = document.createElement("li");
      empty.className = "empty";
      empty.textContent = "No matches. Try “install” or “security”.";
      paletteList.appendChild(empty);
      paletteInput.removeAttribute("aria-activedescendant");
      return;
    }
    filtered.forEach(function (entry, i) {
      var li = document.createElement("li");
      li.id = "pal-opt-" + i;
      li.setAttribute("role", "option");
      li.setAttribute("aria-selected", i === 0 ? "true" : "false");
      var icon = document.createElement("span");
      icon.className = "pal-icon";
      icon.textContent = entry.href.charAt(0) === "#" ? "§" : "↗";
      var label = document.createElement("span");
      label.textContent = entry.label;
      var group = document.createElement("span");
      group.className = "pal-group";
      group.textContent = entry.group;
      li.appendChild(icon);
      li.appendChild(label);
      li.appendChild(group);
      li.addEventListener("click", function () { go(entry); });
      li.addEventListener("mousemove", function () { select(i); });
      paletteList.appendChild(li);
    });
    paletteInput.setAttribute("aria-activedescendant", "pal-opt-0");
  }
  function select(i) {
    if (!filtered.length) return;
    selected = (i + filtered.length) % filtered.length;
    $$("#palette-list li").forEach(function (li, idx) {
      li.setAttribute("aria-selected", idx === selected ? "true" : "false");
    });
    paletteInput.setAttribute("aria-activedescendant", "pal-opt-" + selected);
    var el = document.getElementById("pal-opt-" + selected);
    if (el && el.scrollIntoView) el.scrollIntoView({ block: "nearest" });
  }
  function go(entry) {
    if (!entry) return;
    if (palette && palette.open) palette.close();
    if (entry.href.charAt(0) === "#") {
      var target = document.querySelector(entry.href);
      if (target) {
        target.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "start" });
        history.replaceState(null, "", entry.href);
      }
    } else {
      window.open(entry.href, "_blank", "noopener");
    }
  }
  if (palette && paletteInput && paletteList && typeof palette.showModal === "function") {
    paletteOpen && paletteOpen.addEventListener("click", function () {
      palette.showModal();
      paletteInput.value = "";
      renderPalette();
      paletteInput.focus();
    });
    palette.addEventListener("click", function (e) { if (e.target === palette) palette.close(); });
    paletteInput.addEventListener("input", renderPalette);
    paletteInput.addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown") { e.preventDefault(); select(selected + 1); }
      else if (e.key === "ArrowUp") { e.preventDefault(); select(selected - 1); }
      else if (e.key === "Enter") { e.preventDefault(); go(filtered[selected]); }
    });
    document.addEventListener("keydown", function (e) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        if (palette.open) palette.close();
        else { palette.showModal(); paletteInput.value = ""; renderPalette(); paletteInput.focus(); }
      }
    });
  } else if (paletteOpen) {
    paletteOpen.hidden = true;
  }

  /* ---------- live GitHub data ---------- */
  var REPO = "aryankahar31/runx";
  var CACHE_KEY = "runx-gh-v1";
  function fmtStars(n) {
    if (typeof n !== "number") return "—";
    return n.toLocaleString("en-US");
  }
  function applyStars(n) {
    $$("[data-repo-stars]").forEach(function (el) {
      el.textContent = fmtStars(n);
      if (el.getAttribute("aria-label")) el.setAttribute("aria-label", fmtStars(n) + " GitHub stars");
    });
  }
  function applyVersion(tag) {
    if (!tag) return;
    $$("[data-release-version]").forEach(function (el) { el.textContent = tag; });
    $$(".release-link").forEach(function (a) { a.href = "https://github.com/aryankahar31/runx/releases/tag/" + encodeURIComponent(tag); });
  }
  function readCache() {
    try {
      var raw = sessionStorage.getItem(CACHE_KEY);
      if (!raw) return null;
      var parsed = JSON.parse(raw);
      if (!parsed || Date.now() - parsed.at > 10 * 60 * 1000) return null;
      return parsed;
    } catch (e) { return null; }
  }
  function writeCache(data) {
    try { sessionStorage.setItem(CACHE_KEY, JSON.stringify(data)); } catch (e) {}
  }
  var cached = readCache();
  if (cached) {
    if (cached.stars != null) applyStars(cached.stars);
    if (cached.tag) applyVersion(cached.tag);
  } else if (window.fetch) {
    var headers = { Accept: "application/vnd.github+json" };
    fetch("/data.json", { cache: "no-store" })
      .then(function (r) { return r.ok ? r.json() : null; })
      .catch(function () { return null; })
      .then(function (file) {
        if (file && file.tag && file.stars != null) {
          applyStars(file.stars);
          applyVersion(file.tag);
          writeCache({ at: Date.now(), stars: file.stars, tag: file.tag });
          return;
        }
        Promise.all([
          fetch("https://api.github.com/repos/" + REPO, { headers: headers }).then(function (r) { return r.ok ? r.json() : null; }).catch(function () { return null; }),
          fetch("https://api.github.com/repos/" + REPO + "/releases/latest", { headers: headers }).then(function (r) { return r.ok ? r.json() : null; }).catch(function () { return null; })
        ]).then(function (results) {
          var repo = results[0];
          var release = results[1];
          var data = {
            at: Date.now(),
            stars: repo && repo.stargazers_count != null ? repo.stargazers_count : file && file.stars,
            tag: release && release.tag_name ? release.tag_name : file && file.tag
          };
          if (data.stars != null) applyStars(data.stars);
          if (data.tag) applyVersion(data.tag);
          if (data.stars != null || data.tag) writeCache(data);
        });
      });
  }

  /* ---------- year ---------- */
  $$("[data-year]").forEach(function (el) { el.textContent = String(new Date().getFullYear()); });
})();
