#!/usr/bin/env node
"use strict";

// npm wrapper: fetch the prebuilt runx release binary for this platform,
// verify it against the release's SHA256SUMS, cache it under ~/.runx, then
// hand over to it. No install scripts, no global writes outside the cache.
const { spawnSync } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const REPO = "https://github.com/aryankahar31/runx";
const VERSION_PATTERN = /^\d+\.\d+\.\d+$/;

function validateVersion(version, source = "RUNX_VERSION") {
  const match = typeof version === "string" ? VERSION_PATTERN.exec(version) : null;
  if (
    !match ||
    match[0] !== version ||
    version.includes("/") ||
    version.includes("\\") ||
    version.includes("..")
  ) {
    throw new Error(`${source} must be a numeric version in MAJOR.MINOR.PATCH form (for example 0.7.0)`);
  }
  return version;
}

function platformAsset(platform = process.platform, arch = process.arch) {
  if (platform === "linux" && arch === "x64") return "runx-linux-x64.tar.gz";
  if (platform === "linux" && arch === "arm64") return "runx-linux-arm64.tar.gz";
  if (platform === "darwin" && arch === "x64") return "runx-macos-x64.tar.gz";
  if (platform === "darwin" && arch === "arm64") return "runx-macos-arm64.tar.gz";
  if (platform === "win32" && arch === "x64") return "runx-windows-x64.zip";
  throw new Error(`runx does not publish a binary for ${platform}/${arch}`);
}

function parseChecksum(manifest, asset) {
  for (const line of manifest.split(/\r?\n/)) {
    const match = line.trim().match(/^([0-9a-fA-F]{64})\s+\*?(.+)$/);
    if (match && match[2] === asset) return match[1].toLowerCase();
  }
  throw new Error(`SHA256SUMS has no entry for ${asset}`);
}

async function download(url, dest) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) throw new Error(`GET ${url} failed: HTTP ${response.status}`);
  fs.writeFileSync(dest, Buffer.from(await response.arrayBuffer()));
}

function extract(archive, dir, platform) {
  const result =
    platform === "win32"
      ? spawnSync(
          "powershell",
          ["-NoProfile", "-Command", `Expand-Archive -LiteralPath '${archive}' -DestinationPath '${dir}' -Force`],
          { stdio: "inherit" },
        )
      : spawnSync("tar", ["-xzf", archive, "-C", dir], { stdio: "inherit" });
  if (result.status !== 0) throw new Error(`failed to extract ${archive}`);
}

function cacheDir(version) {
  const home = process.env.RUNX_HOME || path.join(os.homedir(), ".runx");
  return path.join(home, "npm", version);
}

async function ensureBinary(version) {
  const asset = platformAsset();
  const binary = path.join(cacheDir(version), process.platform === "win32" ? "runx.exe" : "runx");
  if (fs.existsSync(binary)) return binary;

  const base = `${REPO}/releases/download/v${version}`;
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "runx-npm-"));
  try {
    const archive = path.join(work, asset);
    await download(`${base}/${asset}`, archive);
    const manifest = await (await fetch(`${base}/SHA256SUMS`, { redirect: "follow" })).text();
    const expected = parseChecksum(manifest, asset);
    const actual = crypto.createHash("sha256").update(fs.readFileSync(archive)).digest("hex");
    if (actual !== expected) {
      throw new Error(`SHA-256 mismatch for ${asset}: expected ${expected}, got ${actual}`);
    }
    const extracted = path.join(work, "out");
    fs.mkdirSync(extracted);
    extract(archive, extracted, process.platform);
    const source = path.join(extracted, process.platform === "win32" ? "runx.exe" : "runx");
    if (!fs.existsSync(source)) throw new Error(`archive did not contain ${path.basename(source)}`);
    fs.mkdirSync(path.dirname(binary), { recursive: true });
    fs.copyFileSync(source, binary);
    fs.chmodSync(binary, 0o755);
    return binary;
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

async function main() {
  const pkg = require("../package.json");
  const version = process.env.RUNX_VERSION === undefined
    ? validateVersion(pkg.version, "package version")
    : validateVersion(process.env.RUNX_VERSION);
  const binary = await ensureBinary(version);
  const child = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
  if (child.error) throw child.error;
  process.exit(child.status === null ? 1 : child.status);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(`runx: ${error.message}`);
    process.exit(1);
  });
}

module.exports = { platformAsset, parseChecksum, validateVersion };
