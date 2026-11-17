import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const REPO = "aryankahar31/runx";
const out = fileURLToPath(new URL("../data.json", import.meta.url));
const headers = { Accept: "application/vnd.github+json", "User-Agent": "runx-website-build" };
if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;

const get = (url) => fetch(url, { headers }).then((r) => (r.ok ? r.json() : null)).catch(() => null);

const [repo, release] = await Promise.all([
  get(`https://api.github.com/repos/${REPO}`),
  get(`https://api.github.com/repos/${REPO}/releases/latest`)
]);

const data = {
  fetchedAt: new Date().toISOString(),
  stars: repo && typeof repo.stargazers_count === "number" ? repo.stargazers_count : null,
  tag: release && typeof release.tag_name === "string" ? release.tag_name : null
};

if (data.stars == null && data.tag == null) {
  console.warn("GitHub data unavailable during build; writing empty data.json for client-side fallback.");
}
writeFileSync(out, JSON.stringify(data, null, 2) + "\n");
console.log(`website data: ${data.tag || "no tag"} · ${data.stars ?? "no stars"}`);
