#!/usr/bin/env bash
#
# Assemble Piki.app from a built piki-gui binary.
#
# Usage:
#   scripts/bundle-macos.sh [path-to-piki-gui-binary] [output-dir]
#
# Defaults:
#   binary     target/release/piki-gui
#   output-dir target/macos
#
# Produces <output-dir>/Piki.app with the proper name and icon so macOS shows
# "Piki" (not "piki-gui") in the Dock, Finder and ⌘-Tab switcher.
#
set -euo pipefail

cd "$(dirname "$0")/.."

BIN="${1:-target/release/piki-gui}"
OUT_DIR="${2:-target/macos}"
APP="$OUT_DIR/Piki.app"

if [[ ! -f "$BIN" ]]; then
  echo "error: binary not found at '$BIN'" >&2
  echo "build it first, e.g. 'cargo build --release -p piki-gui'" >&2
  exit 1
fi

if [[ ! -f assets/piki.icns ]]; then
  echo "error: assets/piki.icns not found; run scripts/gen-icons.sh first" >&2
  exit 1
fi

VERSION="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' gui/Cargo.toml | head -1)"
VERSION="${VERSION:-0.0.0}"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$BIN" "$APP/Contents/MacOS/piki-gui"
chmod +x "$APP/Contents/MacOS/piki-gui"
cp assets/piki.icns "$APP/Contents/Resources/piki.icns"
sed "s/__VERSION__/$VERSION/g" assets/macos/Info.plist > "$APP/Contents/Info.plist"

echo "Created $APP (version $VERSION)"
