#!/usr/bin/env bash
# Build an unsigned NetFlash.app (menu-bar accessory) and zip it for GitHub Releases.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(awk '/^\[workspace.package\]/{p=1} p&&/^version = /{gsub(/"/,"",$3); print $3; exit}' Cargo.toml)"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
bin="$target_dir/release/netflash"
dist="$root/dist"
app="$dist/NetFlash.app"

echo "building netflash $version (release)"
cargo build --release -p netflash-app

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/{{VERSION}}/${version}/g" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
cp "$bin" "$app/Contents/MacOS/netflash"
chmod +x "$app/Contents/MacOS/netflash"

icon_src="$root/packaging/macos/AppIcon.png"
iconset="$root/dist/AppIcon.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
sips -z 16 16     "$icon_src" --out "$iconset/icon_16x16.png" >/dev/null
sips -z 32 32     "$icon_src" --out "$iconset/icon_16x16@2x.png" >/dev/null
sips -z 32 32     "$icon_src" --out "$iconset/icon_32x32.png" >/dev/null
sips -z 64 64     "$icon_src" --out "$iconset/icon_32x32@2x.png" >/dev/null
sips -z 128 128   "$icon_src" --out "$iconset/icon_128x128.png" >/dev/null
sips -z 256 256   "$icon_src" --out "$iconset/icon_128x128@2x.png" >/dev/null
sips -z 256 256   "$icon_src" --out "$iconset/icon_256x256.png" >/dev/null
sips -z 512 512   "$icon_src" --out "$iconset/icon_256x256@2x.png" >/dev/null
sips -z 512 512   "$icon_src" --out "$iconset/icon_512x512.png" >/dev/null
sips -z 1024 1024 "$icon_src" --out "$iconset/icon_512x512@2x.png" >/dev/null
/usr/bin/iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$iconset"

/usr/bin/codesign --force --deep --sign - "$app"

zip="$dist/NetFlash-${version}-macos.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"

echo "app $app"
echo "zip $zip"
