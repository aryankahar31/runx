import { readFileSync, existsSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const errors = [];
const warnings = [];
const fail = (message) => errors.push(message);
const warn = (message) => warnings.push(message);
const read = (file) => readFileSync(join(root, file), "utf8");

const required = [
  "index.html",
  "404.html",
  "style.css",
  "script.js",
  "vercel.json",
  "robots.txt",
  "sitemap.xml",
  "site.webmanifest",
  "favicon.svg",
  "favicon-32.png",
  "apple-touch-icon.png",
  "icon-192.png",
  "og.png",
  "assets/fonts/inter-latin.woff2",
  "assets/fonts/jetbrains-mono-latin.woff2",
  "assets/fonts/OFL.txt"
];
for (const file of required) {
  if (!existsSync(join(root, file))) fail(`missing required file: ${file}`);
}

const html = read("index.html");
const notFound = read("404.html");

for (const [name, body] of [["index.html", html], ["404.html", notFound]]) {
  for (const tag of ["title", "meta name=\"description\"", "meta name=\"viewport\"", "link rel=\"stylesheet\""]) {
    if (!body.includes(tag)) fail(`${name}: missing ${tag}`);
  }
  const ids = new Set([...body.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]));
  for (const match of body.matchAll(/href="#([^"]+)"/g)) {
    if (!ids.has(match[1])) fail(`${name}: anchor #${match[1]} has no matching id`);
  }
  for (const match of body.matchAll(/(?:href|src)="(\/[^"#?]*)/g)) {
    const url = match[1];
    if (url === "/") continue;
    const file = url.endsWith("/") ? `${url.slice(1)}index.html` : url.slice(1);
    if (!existsSync(join(root, file))) fail(`${name}: local reference not found: ${url}`);
  }
  if (/lorem|TODO|FIXME|osrdemo|example\.com/i.test(body)) warn(`${name}: placeholder-looking text found`);
}

for (const marker of [
  "https://runx-cli.vercel.app/",
  "application/ld+json",
  "og:image",
  "twitter:card",
  "data-repo-stars",
  "data-release-version"
]) {
  if (!html.includes(marker)) fail(`index.html: missing ${marker}`);
}
if (!html.includes("Illustrative output")) fail("index.html: demo output must be labelled illustrative");
if (!html.includes("Where runx is weaker")) fail("index.html: comparison must include weaknesses");
if (html.includes("0.2ms launcher overhead · 0ms prompt latency")) fail("index.html: stale hero stat claim present");
if (/\bv0\.6\.\d+\b/.test(html)) fail("index.html: hardcoded current version found; version must come from GitHub API");

for (const file of ["vercel.json", "site.webmanifest", "sitemap.xml"]) {
  if (!existsSync(join(root, file))) continue;
  const body = read(file);
  if (file.endsWith(".json")) {
    try { JSON.parse(body); } catch (e) { fail(`${file}: invalid JSON (${e.message})`); }
  }
  if (file === "sitemap.xml" && !body.includes("https://runx-cli.vercel.app/")) fail("sitemap.xml: missing canonical URL");
}
if (!read("robots.txt").includes("Sitemap: https://runx-cli.vercel.app/sitemap.xml")) fail("robots.txt: missing sitemap");

if (statSync(join(root, "og.png")).size < 10_000) fail("og.png: suspiciously small");

if (warnings.length) for (const w of warnings) console.warn(`warn  ${w}`);
if (errors.length) {
  for (const e of errors) console.error(`error ${e}`);
  console.error(`\n${errors.length} error(s).`);
  process.exit(1);
}
console.log(`website checks passed (${required.length} files, ${warnings.length} warning(s))`);
