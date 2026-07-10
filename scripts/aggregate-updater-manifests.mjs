#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function aggregateUpdaterManifests(directory, options = {}) {
  const dir = resolve(directory);
  const expectedTag = options.expectedTag ?? process.env.EXPECTED_RELEASE_TAG;
  const expectedChannel = options.expectedChannel ?? process.env.EXPECTED_CHANNEL;
  if (!expectedTag) throw new Error("EXPECTED_RELEASE_TAG is required");
  if (!new Set(["stable", "nightly"]).has(expectedChannel)) {
    throw new Error("EXPECTED_CHANNEL must be stable or nightly");
  }
  if (expectedChannel === "stable" && !/^v\d+\.\d+\.\d+$/.test(expectedTag)) {
    throw new Error(`stable updater tag must be vMAJOR.MINOR.PATCH: ${expectedTag}`);
  }
  if (expectedChannel === "nightly" && expectedTag !== "nightly") {
    throw new Error(`nightly updater tag must be nightly: ${expectedTag}`);
  }

  const fragmentNames = readdirSync(dir)
    .filter((name) => /^updater-[a-z0-9_-]+\.json$/.test(name))
    .sort();
  if (fragmentNames.length === 0) throw new Error("no updater fragments found");

  let version;
  let pubDate;
  let notes;
  const platforms = {};
  const verifiedAssets = new Set();
  for (const fragmentName of fragmentNames) {
    const fragment = JSON.parse(readFileSync(resolve(dir, fragmentName), "utf8"));
    if (typeof fragment.version !== "string" || fragment.version.length === 0) {
      throw new Error(`${fragmentName}: missing version`);
    }
    if (version && version !== fragment.version) {
      throw new Error(`${fragmentName}: version ${fragment.version} does not match ${version}`);
    }
    version ??= fragment.version;
    pubDate = laterIsoDate(pubDate, fragment.pub_date, fragmentName);
    notes ??= fragment.notes;
    if (!fragment.platforms || typeof fragment.platforms !== "object") {
      throw new Error(`${fragmentName}: missing platforms`);
    }

    for (const [platform, entry] of Object.entries(fragment.platforms)) {
      if (Object.hasOwn(platforms, platform)) {
        throw new Error(`${fragmentName}: duplicate updater platform ${platform}`);
      }
      if (!entry || typeof entry.url !== "string" || typeof entry.signature !== "string") {
        throw new Error(`${fragmentName}: invalid updater entry for ${platform}`);
      }
      const signature = entry.signature.trim();
      if (signature.length === 0) {
        throw new Error(`${fragmentName}: empty updater signature for ${platform}`);
      }
      if (signature !== entry.signature) {
        throw new Error(`${fragmentName}: updater signature is not canonical for ${platform}`);
      }
      const url = new URL(entry.url);
      const expectedPath = `/releases/download/${expectedTag}/`;
      if (url.hostname !== "github.com" || !url.pathname.includes(expectedPath)) {
        throw new Error(`${fragmentName}: updater URL is outside ${expectedChannel} tag ${expectedTag}`);
      }
      const assetName = basename(decodeURIComponent(url.pathname));
      const assetPath = resolve(dir, assetName);
      const signaturePath = `${assetPath}.sig`;
      if (!assetName || !existsSync(assetPath)) {
        throw new Error(`${fragmentName}: updater asset is missing: ${assetName}`);
      }
      if (!existsSync(signaturePath)) {
        throw new Error(`${fragmentName}: updater signature asset is missing: ${assetName}.sig`);
      }
      const detachedSignature = readFileSync(signaturePath, "utf8").trim();
      if (detachedSignature !== signature) {
        throw new Error(`${fragmentName}: updater signature does not match ${assetName}.sig`);
      }
      if (options.verifyArtifact && !verifiedAssets.has(assetName)) {
        options.verifyArtifact(assetPath, signaturePath);
        verifiedAssets.add(assetName);
      }
      platforms[platform] = { url: entry.url, signature };
    }
  }

  const manifest = {
    version,
    ...(notes ? { notes } : {}),
    pub_date: pubDate,
    platforms,
  };
  writeFileSync(resolve(dir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

function verifyUpdaterArtifact(artifactPath, signaturePath) {
  if (!process.env.TAURI_UPDATER_PUBKEY?.trim()) {
    throw new Error("TAURI_UPDATER_PUBKEY is required to verify updater assets");
  }
  const manifestPath = resolve(
    dirname(fileURLToPath(import.meta.url)),
    "..",
    "tools",
    "updater-signature-verifier",
    "Cargo.toml",
  );
  const result = spawnSync(
    "cargo",
    ["run", "--locked", "--quiet", "--manifest-path", manifestPath],
    {
      stdio: "inherit",
      env: {
        ...process.env,
        TYPEX_UPDATER_ARTIFACT: artifactPath,
        TYPEX_UPDATER_SIGNATURE: signaturePath,
      },
    },
  );
  if (result.error) {
    throw new Error(`failed to start updater signature verifier: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`updater signature verification failed for ${basename(artifactPath)}`);
  }
}

function laterIsoDate(current, candidate, source) {
  if (typeof candidate !== "string" || Number.isNaN(Date.parse(candidate))) {
    throw new Error(`${source}: invalid pub_date`);
  }
  if (!current) return candidate;
  return Date.parse(candidate) > Date.parse(current) ? candidate : current;
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  const directory = process.argv[2];
  if (!directory) {
    console.error("usage: aggregate-updater-manifests.mjs <asset-directory>");
    process.exit(2);
  }
  aggregateUpdaterManifests(directory, { verifyArtifact: verifyUpdaterArtifact });
}
