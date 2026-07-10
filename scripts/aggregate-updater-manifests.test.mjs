import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { aggregateUpdaterManifests } from "./aggregate-updater-manifests.mjs";

function fixture(t) {
  const dir = mkdtempSync(join(tmpdir(), "typex-updater-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "mac.tar.gz"), "mac");
  writeFileSync(join(dir, "mac.tar.gz.sig"), "mac-signature");
  writeFileSync(join(dir, "win-setup.exe"), "win");
  writeFileSync(join(dir, "win-setup.exe.sig"), "win-signature");
  writeFileSync(
    join(dir, "updater-macos.json"),
    JSON.stringify({
      version: "1.2.3",
      pub_date: "2026-07-10T00:00:00Z",
      platforms: {
        "darwin-aarch64": {
          url: "https://github.com/typex-ink/Typex/releases/download/v1.2.3/mac.tar.gz",
          signature: "mac-signature",
        },
      },
    }),
  );
  writeFileSync(
    join(dir, "updater-windows.json"),
    JSON.stringify({
      version: "1.2.3",
      pub_date: "2026-07-10T00:00:01Z",
      platforms: {
        "windows-x86_64": {
          url: "https://github.com/typex-ink/Typex/releases/download/v1.2.3/win-setup.exe",
          signature: "win-signature",
        },
      },
    }),
  );
  return dir;
}

test("aggregates unique platform fragments", (t) => {
  const verified = [];
  const manifest = aggregateUpdaterManifests(fixture(t), {
    expectedTag: "v1.2.3",
    expectedChannel: "stable",
    verifyArtifact: (artifact, signature) => {
      verified.push([artifact.split(/[\\/]/).at(-1), signature.split(/[\\/]/).at(-1)]);
    },
  });
  assert.equal(manifest.version, "1.2.3");
  assert.equal(manifest.pub_date, "2026-07-10T00:00:01Z");
  assert.deepEqual(Object.keys(manifest.platforms).sort(), [
    "darwin-aarch64",
    "windows-x86_64",
  ]);
  assert.deepEqual(verified, [
    ["mac.tar.gz", "mac.tar.gz.sig"],
    ["win-setup.exe", "win-setup.exe.sig"],
  ]);
});

test("rejects detached signatures that differ from the manifest", (t) => {
  const dir = fixture(t);
  writeFileSync(join(dir, "win-setup.exe.sig"), "different-signature");
  assert.throws(
    () =>
      aggregateUpdaterManifests(dir, {
        expectedTag: "v1.2.3",
        expectedChannel: "stable",
      }),
    /updater signature does not match/,
  );
});

test("rejects non-canonical manifest signatures", (t) => {
  const dir = fixture(t);
  const path = join(dir, "updater-windows.json");
  const fragment = JSON.parse(readFileSync(path, "utf8"));
  fragment.platforms["windows-x86_64"].signature = " win-signature\n";
  writeFileSync(path, JSON.stringify(fragment));
  assert.throws(
    () =>
      aggregateUpdaterManifests(dir, {
        expectedTag: "v1.2.3",
        expectedChannel: "stable",
      }),
    /updater signature is not canonical/,
  );
});

test("rejects duplicate platform keys", (t) => {
  const dir = fixture(t);
  const path = join(dir, "updater-windows.json");
  const fragment = JSON.parse(readFileSync(path, "utf8"));
  fragment.platforms = {
    "darwin-aarch64": {
      url: "https://github.com/typex-ink/Typex/releases/download/v1.2.3/win-setup.exe",
      signature: "duplicate",
    },
  };
  writeFileSync(path, JSON.stringify(fragment));
  assert.throws(
    () =>
      aggregateUpdaterManifests(dir, {
        expectedTag: "v1.2.3",
        expectedChannel: "stable",
      }),
    /duplicate updater platform/,
  );
});

test("rejects cross-channel URLs and empty signatures", (t) => {
  const dir = fixture(t);
  const path = join(dir, "updater-windows.json");
  const fragment = JSON.parse(readFileSync(path, "utf8"));
  fragment.platforms["windows-x86_64"].url =
    "https://github.com/typex-ink/Typex/releases/download/nightly/win-setup.exe";
  writeFileSync(path, JSON.stringify(fragment));
  assert.throws(
    () =>
      aggregateUpdaterManifests(dir, {
        expectedTag: "v1.2.3",
        expectedChannel: "stable",
      }),
    /outside stable tag/,
  );
  fragment.platforms["windows-x86_64"].url =
    "https://github.com/typex-ink/Typex/releases/download/v1.2.3/win-setup.exe";
  fragment.platforms["windows-x86_64"].signature = "";
  writeFileSync(path, JSON.stringify(fragment));
  assert.throws(
    () =>
      aggregateUpdaterManifests(dir, {
        expectedTag: "v1.2.3",
        expectedChannel: "stable",
      }),
    /empty updater signature/,
  );
});
