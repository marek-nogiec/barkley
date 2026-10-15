#!/bin/sh
# Regenerate assets/AppIcon.png + AppIcon.icns from assets/AppIcon.svg.
# Needs: rsvg-convert (brew install librsvg) and iconutil (Xcode CLT).
set -eu
cd "$(dirname "$0")/.."
set=$(mktemp -d)/AppIcon.iconset
mkdir -p "$set"
for s in 16 32 128 256 512; do
  rsvg-convert -w $s -h $s assets/AppIcon.svg -o "$set/icon_${s}x${s}.png"
  rsvg-convert -w $((s*2)) -h $((s*2)) assets/AppIcon.svg -o "$set/icon_${s}x${s}@2x.png"
done
rsvg-convert -w 1024 -h 1024 assets/AppIcon.svg -o assets/AppIcon.png
iconutil -c icns "$set" -o assets/AppIcon.icns
echo "wrote assets/AppIcon.icns"
