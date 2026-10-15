#!/bin/sh
# Build target/Barkley.app (menu-bar only: LSUIElement=true).
# Usage: scripts/bundle.sh [--debug]
set -eu
cd "$(dirname "$0")/.."

profile=release
flag=--release
if [ "${1:-}" = "--debug" ]; then profile=debug; flag=; fi

# The app plus every Rust plugin (plugins/*/Cargo.toml).
packages="-p barkley-app"
for toml in plugins/*/Cargo.toml; do
  packages="$packages -p $(sed -n 's/^name = "\(.*\)"/\1/p' "$toml" | head -1)"
done
cargo build $flag $packages

app=target/Barkley.app
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/barkley" "$app/Contents/MacOS/Barkley"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"

# Bundled plugins → Contents/Resources/plugins/<name>: manifest + scripts/assets,
# and for Rust plugins the compiled binary instead of the sources.
# plugins/example-* are templates, not shipped.
for dir in plugins/*/; do
  name=$(basename "$dir")
  case "$name" in example-*) continue ;; esac
  [ -f "$dir/manifest.toml" ] || continue
  dest="$app/Contents/Resources/plugins/$name"
  mkdir -p "$dest"
  rsync -a --exclude src --exclude tests --exclude Cargo.toml --exclude target "$dir" "$dest/"
  if [ -f "$dir/Cargo.toml" ]; then
    bin=$(sed -n 's/^name = "\(.*\)"/\1/p' "$dir/Cargo.toml" | head -1)
    cp "target/$profile/$bin" "$dest/$bin"
  fi
done

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Barkley</string>
  <key>CFBundleDisplayName</key><string>Barkley</string>
  <key>CFBundleIdentifier</key><string>dev.barkley.Barkley</string>
  <key>CFBundleExecutable</key><string>Barkley</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>26.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>The Empty Trash plugin asks Finder how many items are in the Trash and empties it when you choose it.</string>
  <key>NSBluetoothAlwaysUsageDescription</key><string>The Audio plugin lists your paired Bluetooth headphones and connects them when you pick one.</string>
</dict>
</plist>
PLIST

# Sign with a stable identity so macOS keeps the Accessibility grant across rebuilds.
# Priority: $BARKLEY_SIGN_ID (e.g. "Apple Development: …") → the "Barkley Dev"
# identity from scripts/dev-cert.sh → ad-hoc (grant is lost on every rebuild).
dev_keychain="$HOME/Library/Keychains/barkley-dev.keychain-db"
if [ -n "${BARKLEY_SIGN_ID:-}" ]; then
  sign() { codesign --force --sign "$BARKLEY_SIGN_ID" --identifier "$2" "$1"; }
elif [ -f "$dev_keychain" ]; then
  security unlock-keychain -p "" "$dev_keychain"
  id=$(security find-certificate -c "Barkley Dev" -Z "$dev_keychain" | sed -n 's/^SHA-1 hash: //p')
  sign() { codesign --force --keychain "$dev_keychain" --sign "$id" --identifier "$2" "$1"; }
else
  echo "warning: ad-hoc signing; run scripts/dev-cert.sh once to keep Accessibility across rebuilds" >&2
  sign() { codesign --force --sign - --identifier "$2" "$1"; }
fi
# Nested code first: plugin binaries, then the app seals everything.
for f in "$app"/Contents/Resources/plugins/*/*; do
  if [ -f "$f" ] && file -b "$f" | grep -q Mach-O; then
    sign "$f" "dev.barkley.plugin.$(basename "$(dirname "$f")")"
  fi
done
sign "$app" dev.barkley.Barkley
echo "built $app ($(du -sh "$app/Contents/MacOS/Barkley" | cut -f1)); plugins: $(ls "$app/Contents/Resources/plugins" | tr '\n' ' ')"
