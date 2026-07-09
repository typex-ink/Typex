#!/usr/bin/env bash
set -euo pipefail

: "${ASSET_DIR:?ASSET_DIR is required}"
: "${DMG_NAME:?DMG_NAME is required}"
: "${RELEASE_TAG:?RELEASE_TAG is required}"
: "${UPDATER_VERSION:?UPDATER_VERSION is required}"
: "${BUILD_TIME:?BUILD_TIME is required}"

APP_NAME="${APP_NAME:-Typex}"
PLATFORM_ID="${PLATFORM_ID:-macos-universal}"
UPDATE_NOTES="${UPDATE_NOTES:-}"
UPDATER_ARCHIVE_NAME="${UPDATER_ARCHIVE_NAME:-}"

mkdir -p "$ASSET_DIR"

APP_PATH="$(find src-tauri/target -path "*/bundle/macos/${APP_NAME}.app" -type d | sort | head -n 1)"
if [ -z "$APP_PATH" ]; then
  echo "No ${APP_NAME}.app found under src-tauri/target"
  find src-tauri/target -path '*/bundle/*' -type d | sort
  exit 1
fi

rm -rf dmg-root
mkdir -p dmg-root
ditto "$APP_PATH" "dmg-root/${APP_NAME}.app"
ln -s /Applications dmg-root/Applications
hdiutil create \
  -volname "$APP_NAME" \
  -srcfolder dmg-root \
  -ov \
  -format UDZO \
  "$ASSET_DIR/$DMG_NAME"

ASSETS_JSON="[\"${DMG_NAME}\"]"

if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && \
  [ -n "${TAURI_UPDATER_PUBKEY:-}" ] && \
  [ -n "${UPDATER_ARCHIVE_NAME}" ]; then
  tar -C "$(dirname "$APP_PATH")" -czf "$ASSET_DIR/$UPDATER_ARCHIVE_NAME" "$(basename "$APP_PATH")"
  pnpm tauri signer sign "$ASSET_DIR/$UPDATER_ARCHIVE_NAME"

  SIGNATURE="$(cat "$ASSET_DIR/$UPDATER_ARCHIVE_NAME.sig")"
  UPDATER_URL="https://github.com/${GITHUB_REPOSITORY}/releases/download/${RELEASE_TAG}/${UPDATER_ARCHIVE_NAME}"

  ASSET_DIR="$ASSET_DIR" \
    UPDATER_VERSION="$UPDATER_VERSION" \
    UPDATE_NOTES="$UPDATE_NOTES" \
    BUILD_TIME="$BUILD_TIME" \
    UPDATER_URL="$UPDATER_URL" \
    SIGNATURE="$SIGNATURE" \
    node <<'NODE'
const fs = require("fs");

const platform = {
  url: process.env.UPDATER_URL,
  signature: process.env.SIGNATURE,
};

const manifest = {
  version: process.env.UPDATER_VERSION,
  notes: process.env.UPDATE_NOTES || undefined,
  pub_date: process.env.BUILD_TIME,
  platforms: {
    "darwin-aarch64-app": platform,
    "darwin-x86_64-app": platform,
    "darwin-aarch64": platform,
    "darwin-x86_64": platform,
  },
};

fs.writeFileSync(
  `${process.env.ASSET_DIR}/latest.json`,
  `${JSON.stringify(manifest, null, 2)}\n`,
);
NODE

  ASSETS_JSON="[\"${DMG_NAME}\",\"${UPDATER_ARCHIVE_NAME}\",\"${UPDATER_ARCHIVE_NAME}.sig\",\"latest.json\"]"
else
  echo "Skipping Tauri updater assets: signing secrets or UPDATER_ARCHIVE_NAME are not configured."
fi

PLATFORM_ID="$PLATFORM_ID" ASSETS_JSON="$ASSETS_JSON" ASSET_DIR="$ASSET_DIR" node <<'NODE'
const fs = require("fs");

const metadata = {
  platform: process.env.PLATFORM_ID,
  assets: JSON.parse(process.env.ASSETS_JSON),
};

fs.writeFileSync(
  `${process.env.ASSET_DIR}/${process.env.PLATFORM_ID}.json`,
  `${JSON.stringify(metadata, null, 2)}\n`,
);
NODE
