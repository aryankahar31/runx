"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const { platformAsset, parseChecksum } = require("./bin/runx.js");

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
