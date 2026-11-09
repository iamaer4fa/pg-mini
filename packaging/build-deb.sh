#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)"
ARCH="$(dpkg --print-architecture)"
STAGING_DIR="$ROOT_DIR/target/pg-mini-deb-root"
PACKAGE_DIR="$ROOT_DIR/target/debian"
PACKAGE_PATH="$PACKAGE_DIR/pg-mini_${VERSION}_${ARCH}.deb"

if [[ -z "$VERSION" ]]; then
    echo "Could not read package version from Cargo.toml" >&2
    exit 1
fi

cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"

install -d -m 0755 \
    "$STAGING_DIR" \
    "$STAGING_DIR/DEBIAN" \
    "$STAGING_DIR/usr" \
    "$STAGING_DIR/usr/bin" \
    "$STAGING_DIR/usr/share" \
    "$STAGING_DIR/usr/share/applications" \
    "$STAGING_DIR/usr/share/icons" \
    "$STAGING_DIR/usr/share/icons/hicolor" \
    "$STAGING_DIR/usr/share/icons/hicolor/scalable" \
    "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps" \
    "$STAGING_DIR/usr/share/icons/hicolor/256x256" \
    "$STAGING_DIR/usr/share/icons/hicolor/256x256/apps"
install -m 0755 "$ROOT_DIR/target/release/pg-mini" "$STAGING_DIR/usr/bin/pg-mini"
install -m 0644 "$ROOT_DIR/packaging/pg-mini.desktop" "$STAGING_DIR/usr/share/applications/pg-mini.desktop"
install -m 0644 "$ROOT_DIR/packaging/pg-mini.svg" "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps/pg-mini.svg"
install -m 0644 "$ROOT_DIR/assets/pg-mini.png" "$STAGING_DIR/usr/share/icons/hicolor/256x256/apps/pg-mini.png"

cat > "$STAGING_DIR/DEBIAN/control" <<EOF
Package: pg-mini
Version: $VERSION
Section: database
Priority: optional
Architecture: $ARCH
Depends: libc6, libgcc-s1, libgl1, libwayland-client0, libx11-6, libxcb1, libxkbcommon0
Maintainer: pg-mini contributors
Description: Lightweight native PostgreSQL manager
 A fast desktop client for browsing PostgreSQL databases and running SQL queries.
EOF

mkdir -p "$PACKAGE_DIR"
dpkg-deb --build --root-owner-group "$STAGING_DIR" "$PACKAGE_PATH"
echo "Built $PACKAGE_PATH"
