#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PATH="${1:-}"
CODE_SIGN_IDENTITY="${TYPEX_CODESIGN_IDENTITY:-${APPLE_SIGNING_IDENTITY:-}}"

if [ -z "$CODE_SIGN_IDENTITY" ]; then
  echo "TYPEX_CODESIGN_IDENTITY (or APPLE_SIGNING_IDENTITY) is required for macOS release signing" >&2
  exit 1
fi

if [ "$CODE_SIGN_IDENTITY" = "-" ]; then
  echo "Ad-hoc signing is not accepted for a Typex release artifact" >&2
  exit 1
fi

if [ -z "$APP_PATH" ]; then
  APP_PATH="$(find "$REPO_ROOT/src-tauri/target" -path '*/bundle/macos/Typex.app' -type d | sort | head -n 1)"
elif [ "${APP_PATH#/}" = "$APP_PATH" ]; then
  APP_PATH="$REPO_ROOT/$APP_PATH"
fi

if [ -z "$APP_PATH" ]; then
  echo "No Typex.app found under src-tauri/target"
  find "$REPO_ROOT/src-tauri/target" -path '*/bundle/*' -type d | sort
  exit 1
fi

if [ ! -d "$APP_PATH" ]; then
  echo "Typex.app does not exist: $APP_PATH" >&2
  exit 1
fi

FRAMEWORKS_DIR="$APP_PATH/Contents/Frameworks"
ENTITLEMENTS="$REPO_ROOT/src-tauri/Entitlements.plist"
mkdir -p "$FRAMEWORKS_DIR"

copy_universal_dylib() {
  name="$1"
  dest="$FRAMEWORKS_DIR/$name"
  universal_src=""
  arm64_src=""
  x86_64_src=""

  while IFS= read -r src; do
    archs="$(lipo -archs "$src" 2>/dev/null || true)"
    if echo "$archs" | grep -qw arm64 && echo "$archs" | grep -qw x86_64; then
      universal_src="$src"
      break
    fi
    if echo "$archs" | grep -qw arm64 && [ -z "$arm64_src" ]; then
      arm64_src="$src"
    fi
    if echo "$archs" | grep -qw x86_64 && [ -z "$x86_64_src" ]; then
      x86_64_src="$src"
    fi
  done < <(
    find "$REPO_ROOT/src-tauri/target" \
      -name "$name" \
      -type f \
      ! -path '*.dSYM/*' \
      ! -path '*/bundle/*' | sort
  )

  if [ -n "$universal_src" ]; then
    cp "$universal_src" "$dest"
  elif [ -n "$arm64_src" ] && [ -n "$x86_64_src" ]; then
    lipo -create "$arm64_src" "$x86_64_src" -output "$dest"
  else
    echo "Missing runtime dylib: $name"
    echo "arm64 source: ${arm64_src:-<none>}"
    echo "x86_64 source: ${x86_64_src:-<none>}"
    exit 1
  fi

  chmod 755 "$dest"
  lipo -archs "$dest"
}

copy_universal_dylib libonnxruntime.1.17.1.dylib
copy_universal_dylib libsherpa-onnx-c-api.dylib

install_name_tool -add_rpath "@executable_path/../Frameworks" "$APP_PATH/Contents/MacOS/typex" 2>/dev/null || true
install_name_tool -add_rpath "@executable_path/../Frameworks" "$FRAMEWORKS_DIR/libsherpa-onnx-c-api.dylib" 2>/dev/null || true

# Remove any outer ad-hoc signature before replacing nested code signatures.
codesign --remove-signature "$APP_PATH" 2>/dev/null || true

sign_macho_tree() {
  root="$1"
  if [ ! -d "$root" ]; then
    return
  fi
  while IFS= read -r -d '' file; do
    if file -b "$file" | grep -q 'Mach-O'; then
      codesign --force --options runtime --timestamp --sign "$CODE_SIGN_IDENTITY" "$file"
    fi
  done < <(find "$root" -type f -print0)
}

# Sign nested Mach-O code first, then sign bundle containers from the inside out.
sign_macho_tree "$APP_PATH/Contents/Frameworks"
sign_macho_tree "$APP_PATH/Contents/PlugIns"
sign_macho_tree "$APP_PATH/Contents/XPCServices"
sign_macho_tree "$APP_PATH/Contents/MacOS"

while IFS= read -r -d '' file; do
  case "$file" in
    "$APP_PATH/Contents/Frameworks/"*|"$APP_PATH/Contents/PlugIns/"*|"$APP_PATH/Contents/XPCServices/"*|"$APP_PATH/Contents/MacOS/"*)
      continue
      ;;
  esac
  if file -b "$file" | grep -q 'Mach-O'; then
    codesign --force --options runtime --timestamp --sign "$CODE_SIGN_IDENTITY" "$file"
  fi
done < <(find "$APP_PATH/Contents" -type f -print0)

while IFS= read -r -d '' bundle; do
  codesign --force --options runtime --timestamp --sign "$CODE_SIGN_IDENTITY" "$bundle"
done < <(
  find "$APP_PATH/Contents" -depth -type d \( \
    -name '*.framework' -o \
    -name '*.bundle' -o \
    -name '*.xpc' \
  \) -print0
)

codesign \
  --force \
  --options runtime \
  --timestamp \
  --entitlements "$ENTITLEMENTS" \
  --sign "$CODE_SIGN_IDENTITY" \
  "$APP_PATH"

codesign --verify --deep --strict --verbose=2 "$APP_PATH"
codesign --display --verbose=2 "$APP_PATH" 2>&1 | sed -n '1,40p'

otool -L "$APP_PATH/Contents/MacOS/typex"
test -f "$FRAMEWORKS_DIR/libonnxruntime.1.17.1.dylib"
test -f "$FRAMEWORKS_DIR/libsherpa-onnx-c-api.dylib"
