#!/bin/sh
# Renders the app icons from the SVG sources in assets/ (macOS only: uses iconutil).
#
# Usage: scripts/icons.sh   (needs resvg: cargo install resvg --locked)
# Writes assets/icon.ico (Windows exe), assets/icon-256.png (Windows window icon)
# and assets/icon.icns (macOS app). The outputs are checked in.
set -eu
cd "$(dirname "$0")/.."

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Windows: one PNG per size, packed into an .ico (PNG entries, Windows Vista and later).
for s in 16 24 32 48 64 256; do
    resvg -w "$s" -h "$s" assets/icon.svg "$tmp/win-$s.png"
done
cp "$tmp/win-256.png" assets/icon-256.png
python3 - "$tmp" assets/icon.ico <<'EOF'
import struct, sys
tmp, out = sys.argv[1], sys.argv[2]
sizes = [16, 24, 32, 48, 64, 256]
pngs = [open(f"{tmp}/win-{s}.png", "rb").read() for s in sizes]
offset = 6 + 16 * len(sizes)
data = struct.pack("<HHH", 0, 1, len(sizes))
for s, png in zip(sizes, pngs):
    data += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(png), offset)
    offset += len(png)
open(out, "wb").write(data + b"".join(pngs))
EOF

# macOS: an .iconset with every size iconutil expects.
set_dir="$tmp/icon.iconset"
mkdir "$set_dir"
for s in 16 32 128 256 512; do
    resvg -w "$s" -h "$s" assets/icon-macos.svg "$set_dir/icon_${s}x${s}.png"
    d=$((s * 2))
    resvg -w "$d" -h "$d" assets/icon-macos.svg "$set_dir/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$set_dir" -o assets/icon.icns

ls -l assets/icon.ico assets/icon-256.png assets/icon.icns
