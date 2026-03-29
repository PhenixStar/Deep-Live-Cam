#!/bin/bash
# Build the Rust server binary as a Tauri sidecar replacement for Python.
# Produces a single binary at app/src-tauri/binaries/deep-forge-server-{TRIPLE}
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BINARIES_DIR="$REPO_ROOT/app/src-tauri/binaries"

TRIPLE=$(rustc -vV | grep 'host:' | awk '{print $2}')

echo "[BUILD] Building Rust server for $TRIPLE..."
cd "$REPO_ROOT/core/rust-engine"
cargo build --release -p deep-forge-server

echo "[BUILD] Copying binary to Tauri binaries..."
mkdir -p "$BINARIES_DIR"
cp "target/release/deep-forge-server" "$BINARIES_DIR/deep-forge-server-${TRIPLE}"
chmod +x "$BINARIES_DIR/deep-forge-server-${TRIPLE}"

echo "[BUILD] Done. Binary: $BINARIES_DIR/deep-forge-server-${TRIPLE}"
echo "[BUILD] Size: $(du -h "target/release/deep-forge-server" | cut -f1)"
