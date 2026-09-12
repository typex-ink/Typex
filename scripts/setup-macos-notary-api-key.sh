#!/usr/bin/env bash
set -euo pipefail

: "${APPLE_NOTARY_KEY_BASE64:?APPLE_NOTARY_KEY_BASE64 is required}"
: "${APPLE_NOTARY_KEY_ID:?APPLE_NOTARY_KEY_ID is required}"
: "${APPLE_NOTARY_ISSUER:?APPLE_NOTARY_ISSUER is required}"

RUNNER_TEMP_DIR="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
API_KEY_PATH="$RUNNER_TEMP_DIR/AuthKey_${APPLE_NOTARY_KEY_ID}.p8"

if ! printf '%s' "$APPLE_NOTARY_KEY_BASE64" | base64 -D > "$API_KEY_PATH" 2>/dev/null; then
  printf '%s' "$APPLE_NOTARY_KEY_BASE64" | base64 --decode > "$API_KEY_PATH"
fi
chmod 600 "$API_KEY_PATH"

if ! grep -q '^-----BEGIN PRIVATE KEY-----' "$API_KEY_PATH"; then
  echo "Apple notarization API key is not a PEM private key" >&2
  exit 1
fi

export APPLE_API_KEY_PATH="$API_KEY_PATH"
export APPLE_API_KEY_ID="$APPLE_NOTARY_KEY_ID"
export APPLE_API_ISSUER="$APPLE_NOTARY_ISSUER"

if [ -n "${GITHUB_ENV:-}" ]; then
  {
    printf 'APPLE_API_KEY_PATH=%s\n' "$APPLE_API_KEY_PATH"
    printf 'APPLE_API_KEY_ID=%s\n' "$APPLE_API_KEY_ID"
    printf 'APPLE_API_ISSUER=%s\n' "$APPLE_API_ISSUER"
  } >> "$GITHUB_ENV"
fi

echo "Configured Apple notarization API key $APPLE_API_KEY_ID"
