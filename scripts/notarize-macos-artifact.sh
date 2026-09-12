#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-}"

if [ -z "$TARGET" ]; then
  TARGET="$(find "$REPO_ROOT/src-tauri/target" -path '*/bundle/macos/Typex.app' -type d | sort | head -n 1)"
elif [ "${TARGET#/}" = "$TARGET" ]; then
  TARGET="$REPO_ROOT/$TARGET"
fi

if [ -z "$TARGET" ] || [ ! -e "$TARGET" ]; then
  echo "macOS notarization target does not exist: ${TARGET:-<empty>}" >&2
  exit 1
fi

case "$TARGET" in
  *.app)
    TARGET_KIND="app"
    ;;
  *.dmg)
    TARGET_KIND="dmg"
    ;;
  *)
    echo "Notarization target must be a .app or .dmg: $TARGET" >&2
    exit 1
    ;;
esac

AUTH_ARGS=()
if [ -n "${TYPEX_NOTARY_KEYCHAIN_PROFILE:-}" ]; then
  AUTH_ARGS+=(--keychain-profile "$TYPEX_NOTARY_KEYCHAIN_PROFILE")
elif [ -n "${APPLE_API_KEY_PATH:-}" ] && \
  [ -n "${APPLE_API_KEY_ID:-}" ] && \
  [ -n "${APPLE_API_ISSUER:-}" ]; then
  if [ ! -f "$APPLE_API_KEY_PATH" ]; then
    echo "APPLE_API_KEY_PATH does not exist: $APPLE_API_KEY_PATH" >&2
    exit 1
  fi
  AUTH_ARGS+=(
    --key "$APPLE_API_KEY_PATH"
    --key-id "$APPLE_API_KEY_ID"
    --issuer "$APPLE_API_ISSUER"
  )
elif [ -n "${APPLE_ID:-}" ] && \
  [ -n "${APPLE_APP_SPECIFIC_PASSWORD:-}" ] && \
  [ -n "${APPLE_TEAM_ID:-}" ]; then
  AUTH_ARGS+=(
    --apple-id "$APPLE_ID"
    --password "$APPLE_APP_SPECIFIC_PASSWORD"
    --team-id "$APPLE_TEAM_ID"
  )
else
  cat >&2 <<'EOF'
No Apple notarization credentials configured. Set TYPEX_NOTARY_KEYCHAIN_PROFILE,
APPLE_API_KEY_PATH + APPLE_API_KEY_ID + APPLE_API_ISSUER, or
APPLE_ID + APPLE_APP_SPECIFIC_PASSWORD + APPLE_TEAM_ID.
EOF
  exit 1
fi

WORK_ROOT="${TYPEX_NOTARY_WORK_ROOT:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}}"
WORK_DIR="$(mktemp -d "$WORK_ROOT/typex-notary.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

SUBMIT_TARGET="$TARGET"
if [ "$TARGET_KIND" = "app" ]; then
  SUBMIT_TARGET="$WORK_DIR/Typex-notarize.zip"
  ditto -c -k --keepParent "$TARGET" "$SUBMIT_TARGET"
fi

xcrun notarytool submit "$SUBMIT_TARGET" "${AUTH_ARGS[@]}" --wait
xcrun stapler staple "$TARGET"
xcrun stapler validate "$TARGET"

echo "Notarized and stapled ($TARGET_KIND): $TARGET"
