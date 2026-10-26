#!/bin/sh
# Builds the macOS release: dist/dropdl.app and dist/dropdl-macos-<arch>.zip
#
# Usage: scripts/bundle-macos.sh
# The app is signed ad hoc (no Apple Developer ID): Gatekeeper asks once on
# first launch (System Settings > Privacy & Security > Open Anyway).
set -eu
cd "$(dirname "$0")/.."

cargo build --release
exe=target/release/dropdl

# Fail on libraries that a clean Mac doesn't have (only system ones are allowed).
extra=$(otool -L "$exe" | tail -n +2 | awk '{print $1}' | grep -vE '^(/usr/lib/|/System/Library/)' || true)
if [ -n "$extra" ]; then
    echo "non-system libraries:" >&2
    echo "$extra" >&2
    exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
arch=$(uname -m)
app=dist/dropdl.app

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$exe" "$app/Contents/MacOS/dropdl"
cp THIRD-PARTY-LICENSES.txt "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>dropdl</string>
    <key>CFBundleDisplayName</key><string>dropdl</string>
    <key>CFBundleIdentifier</key><string>io.github.edarbieto.dropdl</string>
    <key>CFBundleExecutable</key><string>dropdl</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

codesign --force --sign - "$app"
codesign --verify "$app"

zip_path="dist/dropdl-macos-$arch.zip"
rm -f "$zip_path"
ditto -c -k --keepParent "$app" "$zip_path"

bytes() { stat -f %z "$1"; }
echo "$app/Contents/MacOS/dropdl  $(bytes "$app/Contents/MacOS/dropdl") B"
echo "$zip_path  $(bytes "$zip_path") B"
