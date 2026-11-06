#!/usr/bin/env bash
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ ! -f "$SCRIPT_DIR/target/release/pg-mini" ]; then
    echo "Building pg-mini in release mode..."
    cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"
fi

exec "$SCRIPT_DIR/target/release/pg-mini" "$@"
