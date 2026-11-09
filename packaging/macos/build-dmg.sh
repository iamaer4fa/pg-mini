#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="${1:?Usage: build-dmg.sh <rust-target> <architecture>}"
ARCH="${2:?Usage: build-dmg.sh <rust-target> <architecture>}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)"
OUTPUT_DIR="$ROOT_DIR/target/macos"
APP_DIR="$OUTPUT_DIR/pg-mini.app"
ICONSET_DIR="$OUTPUT_DIR/pg-mini.iconset"
DMG_ROOT="$OUTPUT_DIR/dmg-root-$ARCH"
DMG_PATH="$OUTPUT_DIR/pg-mini_${VERSION}_macos_${ARCH}.dmg"

mkdir -p \
    "$APP_DIR/Contents/MacOS" \
    "$APP_DIR/Contents/Resources" \
    "$ICONSET_DIR" \
    "$DMG_ROOT"

cp "$ROOT_DIR/target/$TARGET/release/pg-mini" "$APP_DIR/Contents/MacOS/pg-mini"
chmod 0755 "$APP_DIR/Contents/MacOS/pg-mini"

sips -z 16 16 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_16x16.png" >/dev/null
sips -z 32 32 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_16x16@2x.png" >/dev/null
sips -z 32 32 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_32x32.png" >/dev/null
sips -z 64 64 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_32x32@2x.png" >/dev/null
sips -z 128 128 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_128x128.png" >/dev/null
sips -z 256 256 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null
sips -z 256 256 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_256x256.png" >/dev/null
sips -z 512 512 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null
sips -z 512 512 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_512x512.png" >/dev/null
sips -z 1024 1024 "$ROOT_DIR/assets/pg-mini.png" --out "$ICONSET_DIR/icon_512x512@2x.png" >/dev/null
iconutil -c icns "$ICONSET_DIR" -o "$APP_DIR/Contents/Resources/pg-mini.icns"

cat > "$APP_DIR/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>pg-mini</string>
    <key>CFBundleIconFile</key>
    <string>pg-mini</string>
    <key>CFBundleIdentifier</key>
    <string>com.pgmini.app</string>
    <key>CFBundleName</key>
    <string>pg-mini</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

cp -R "$APP_DIR" "$DMG_ROOT/pg-mini.app"
ln -sfn /Applications "$DMG_ROOT/Applications"
hdiutil create \
    -volname "pg-mini" \
    -srcfolder "$DMG_ROOT" \
    -ov \
    -format UDZO \
    "$DMG_PATH"

echo "Built $DMG_PATH"
