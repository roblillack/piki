#!/usr/bin/env bash
#
# Regenerate Piki's raster icon assets from gui/assets/icon.svg.
#
# Outputs:
#   gui/assets/icon-512.png - embedded in the binary for the macOS Dock icon
#   gui/assets/piki.icns    - bundled into Piki.app on macOS
#
# All icon assets live under gui/assets/ so they ship inside the published
# piki-gui crate (include_str!/include_bytes! can only reach files within the
# crate).
#
# Requirements: rsvg-convert (librsvg). On macOS `iconutil` is used to build
# the .icns; elsewhere the .icns step is skipped.
#
set -euo pipefail

# Run from the workspace root (this script lives in gui/scripts/).
cd "$(dirname "$0")/../.."
SVG="gui/assets/icon.svg"

if ! command -v rsvg-convert >/dev/null 2>&1; then
  echo "error: rsvg-convert not found (install librsvg, e.g. 'brew install librsvg')" >&2
  exit 1
fi

echo "Rendering gui/assets/icon-512.png"
rsvg-convert -w 512 -h 512 "$SVG" -o gui/assets/icon-512.png

if command -v iconutil >/dev/null 2>&1; then
  echo "Building gui/assets/piki.icns"
  ICONSET="$(mktemp -d)/piki.iconset"
  mkdir -p "$ICONSET"
  for size in 16 32 128 256 512; do
    rsvg-convert -w "$size"          -h "$size"          "$SVG" -o "$ICONSET/icon_${size}x${size}.png"
    rsvg-convert -w "$((size * 2))" -h "$((size * 2))" "$SVG" -o "$ICONSET/icon_${size}x${size}@2x.png"
  done
  iconutil -c icns "$ICONSET" -o gui/assets/piki.icns
  rm -rf "$ICONSET"
else
  echo "iconutil not found; skipping gui/assets/piki.icns (run this on macOS to regenerate it)"
fi

echo "Done."
