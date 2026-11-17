"use strict";

const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const path = require("node:path");
const test = require("node:test");
const { platformAsset, parseChecksum, validateVersion } = require("./bin/runx.js");

test("maps every published platform to its release asset", () => {
  assert.equal(platformAsset("linux", "x64"), "runx-linux-x64.tar.gz");
  assert.equal(platformAsset("linux", "arm64"), "runx-linux-arm64.tar.gz");
  assert.equal(platformAsset("darwin", "x64"), "runx-macos-x64.tar.gz");
  assert.equal(platformAsset("darwin", "arm64"), "runx-macos-arm64.tar.gz");
  assert.equal(platformAsset("win32", "x64"), "runx-windows-x64.zip");
  assert.throws(() => platformAsset("freebsd", "x64"), /does not publish/);
});

test("parses a SHA256SUMS entry for the exact asset", () => {
  const hash = "a".repeat(64);
  const other = "b".repeat(64);
  const manifest = `${other}  runx-linux-arm64.tar.gz\n${hash}  runx-linux-x64.tar.gz\n`;
  assert.equal(parseChecksum(manifest, "runx-linux-x64.tar.gz"), hash);
  assert.throws(() => parseChecksum(manifest, "runx-macos-x64.tar.gz"), /no entry/);
});

test("accepts only numeric major.minor.patch versions", () => {
  assert.equal(validateVersion("0.7.0"), "0.7.0");
  for (const payload of [
    "../x",
    "/tmp/x",
    "C:\\Windows\\Temp\\x",
    "1.2.3;touch /tmp/pwn",
    "1.2.3$(whoami)",
    "1.2.3|cat /etc/passwd",
    "1.2.3..x",
    "1.2.3\n",
    "v1.2.3",
    "1.2",
  ]) {
    assert.throws(
      () => validateVersion(payload),
      /RUNX_VERSION must be a numeric version in MAJOR\.MINOR\.PATCH form/,
      `expected ${JSON.stringify(payload)} to be rejected`,
    );
  }
});

test("rejects invalid RUNX_VERSION before starting a download", () => {
  const wrapper = path.join(__dirname, "bin", "runx.js");
  for (const payload of ["../x", "/tmp/x", "1.2.3;touch /tmp/pwn"]) {
    const result = spawnSync(process.execPath, [wrapper, "--version"], {
      encoding: "utf8",
      env: { ...process.env, RUNX_VERSION: payload },
    });
    assert.equal(result.status, 1, `expected ${JSON.stringify(payload)} to be rejected`);
    assert.match(result.stderr, /RUNX_VERSION must be a numeric version/);
  }
});
