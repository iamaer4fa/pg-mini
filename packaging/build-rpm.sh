#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)"
RPM_BUILD_DIR="$ROOT_DIR/target/rpm-build"
PACKAGE_DIR="$ROOT_DIR/target/rpm"

if [[ -z "$VERSION" ]]; then
    echo "Could not read package version from Cargo.toml" >&2
    exit 1
fi

if ! command -v rpmbuild >/dev/null 2>&1; then
    echo "rpmbuild is required to build the RPM package" >&2
    exit 1
fi

cargo build --release --locked --manifest-path "$ROOT_DIR/Cargo.toml"

mkdir -p \
    "$RPM_BUILD_DIR/BUILD" \
    "$RPM_BUILD_DIR/BUILDROOT" \
    "$RPM_BUILD_DIR/RPMS" \
    "$RPM_BUILD_DIR/SOURCES" \
    "$RPM_BUILD_DIR/SPECS" \
    "$RPM_BUILD_DIR/SRPMS" \
    "$PACKAGE_DIR"
install -m 0755 "$ROOT_DIR/target/release/pg-mini" "$RPM_BUILD_DIR/SOURCES/pg-mini"
install -m 0644 "$ROOT_DIR/packaging/pg-mini.desktop" "$RPM_BUILD_DIR/SOURCES/pg-mini.desktop"
install -m 0644 "$ROOT_DIR/packaging/pg-mini.svg" "$RPM_BUILD_DIR/SOURCES/pg-mini.svg"
install -m 0644 "$ROOT_DIR/assets/pg-mini.png" "$RPM_BUILD_DIR/SOURCES/pg-mini.png"
install -m 0644 "$ROOT_DIR/packaging/pg-mini.spec" "$RPM_BUILD_DIR/SPECS/pg-mini.spec"

rpmbuild \
    --define "_topdir $RPM_BUILD_DIR" \
    --define "pkg_version $VERSION" \
    -bb "$RPM_BUILD_DIR/SPECS/pg-mini.spec"

find "$RPM_BUILD_DIR/RPMS" -type f -name "pg-mini-${VERSION}-*.rpm" \
    -exec install -m 0644 {} "$PACKAGE_DIR/" \;

if ! compgen -G "$PACKAGE_DIR/pg-mini-${VERSION}-*.rpm" >/dev/null; then
    echo "RPM build completed without producing a package" >&2
    exit 1
fi

echo "Built RPM package(s) in $PACKAGE_DIR"
