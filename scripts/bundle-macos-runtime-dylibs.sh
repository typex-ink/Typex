#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:-}"
if [ -z "$APP_PATH" ]; then
  APP_PATH="$(find src-tauri/target -path '*/bundle/macos/Typex.app' -type d | sort | head -n 1)"
fi

if [ -z "$APP_PATH" ]; then
  echo "No Typex.app found under src-tauri/target"
  find src-tauri/target -path '*/bundle/*' -type d | sort
  exit 1
fi

FRAMEWORKS_DIR="$APP_PATH/Contents/Frameworks"
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
  done < <(find src-tauri/target -name "$name" -type f ! -path '*.dSYM/*' | sort)

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
codesign --force --deep --sign "${TYPEX_CODESIGN_IDENTITY:--}" --entitlements src-tauri/Entitlements.plist "$APP_PATH"

otool -L "$APP_PATH/Contents/MacOS/typex"
test -f "$FRAMEWORKS_DIR/libonnxruntime.1.17.1.dylib"
test -f "$FRAMEWORKS_DIR/libsherpa-onnx-c-api.dylib"
