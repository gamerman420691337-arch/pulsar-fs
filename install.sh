#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"

echo "=================================================="
echo "Installing Funscript Hub (fs-hub)"
echo "=================================================="

# Check Cargo
if ! command -v cargo &> /dev/null; then
    echo "Error: cargo could not be found. Please install Rust: https://rustup.rs"
    exit 1
fi

echo "-> Building optimized release binary..."
cd "${SCRIPT_DIR}"
cargo build --release --bin fs-hub

mkdir -p "${BIN_DIR}"

TARGET_BIN="${SCRIPT_DIR}/target/release/fs-hub"
if [ ! -f "${TARGET_BIN}" ]; then
    TARGET_BIN="${SCRIPT_DIR}/open-fungen/target/release/fs-hub"
fi

echo "-> Installing binary symlinks into ${BIN_DIR}..."
ln -sf "${TARGET_BIN}" "${BIN_DIR}/fs-hub"
ln -sf "${TARGET_BIN}" "${BIN_DIR}/open-fungen"

echo "=================================================="
echo "Funscript Hub installation complete!"
echo "Target binary: ${TARGET_BIN}"
echo "You can now run:"
echo "  fs-hub         # Open unified desktop GUI"
echo "  fs-hub --help  # Show CLI commands"
echo "=================================================="
