#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"

echo "=================================================="
echo "Installing Pulsar — Funscript Generator (pulsar)"
echo "=================================================="

# Check Cargo
if ! command -v cargo &> /dev/null; then
    echo "Error: cargo could not be found. Please install Rust: https://rustup.rs"
    exit 1
fi

if ! command -v python3 &> /dev/null; then
    echo "Error: Python 3 is required to resolve Cargo's target directory." >&2
    exit 1
fi
if command -v timeout &> /dev/null; then
    TIMEOUT_BIN=timeout
elif command -v gtimeout &> /dev/null; then
    TIMEOUT_BIN=gtimeout
else
    echo "Error: GNU timeout is required (install coreutils; macOS uses gtimeout)." >&2
    exit 1
fi

CARGO_FEATURES=()
if command -v nvidia-smi &> /dev/null; then
    echo "-> NVIDIA GPU detected! Bundling hardware CUDA acceleration..."
    CARGO_FEATURES=(--features cuda)
fi

echo "-> Building optimized release binary..."
cd "${SCRIPT_DIR}"
"${TIMEOUT_BIN}" 1800 cargo build --release --workspace --bins "${CARGO_FEATURES[@]}"

mkdir -p "${BIN_DIR}"

TARGET_DIR="$("${TIMEOUT_BIN}" 60 cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
TARGET_BIN="${TARGET_DIR}/release/pulsar"
WORKER_BIN="${TARGET_DIR}/release/pulsar-worker"
if [ ! -x "${TARGET_BIN}" ] || [ ! -x "${WORKER_BIN}" ]; then
    echo "Error: both pulsar and pulsar-worker are required in ${TARGET_DIR}/release" >&2
    exit 1
fi

echo "-> Installing binary symlinks into ${BIN_DIR}..."
ln -sf "${TARGET_BIN}" "${BIN_DIR}/pulsar"
ln -sf "${WORKER_BIN}" "${BIN_DIR}/pulsar-worker"
ln -sf "${TARGET_BIN}" "${BIN_DIR}/fs-hub"
ln -sf "${TARGET_BIN}" "${BIN_DIR}/open-fungen"

# Install Desktop Entry and Icons (Linux Freedesktop)
if [ -d "${SCRIPT_DIR}/assets" ]; then
    APPS_DIR="${HOME}/.local/share/applications"
    ICONS_DIR="${HOME}/.local/share/icons/hicolor"
    MIME_DIR="${HOME}/.local/share/mime/packages"

    mkdir -p "${APPS_DIR}" "${ICONS_DIR}" "${MIME_DIR}"

    echo "-> Installing desktop launcher and icons..."
    if [ -f "${SCRIPT_DIR}/assets/pulsar.desktop" ]; then
        /usr/bin/cp "${SCRIPT_DIR}/assets/pulsar.desktop" "${APPS_DIR}/pulsar.desktop"
    fi

    if [ -d "${SCRIPT_DIR}/assets/icons/hicolor" ]; then
        /usr/bin/cp -r "${SCRIPT_DIR}/assets/icons/hicolor/"* "${ICONS_DIR}/" 2>/dev/null || true
    fi

    if [ -f "${SCRIPT_DIR}/assets/icons/scalable/apps/pulsar.svg" ]; then
        mkdir -p "${ICONS_DIR}/scalable/apps"
        /usr/bin/cp "${SCRIPT_DIR}/assets/icons/scalable/apps/pulsar.svg" "${ICONS_DIR}/scalable/apps/pulsar.svg"
    fi

    if [ -f "${SCRIPT_DIR}/assets/mime/pulsar-mime.xml" ]; then
        /usr/bin/cp "${SCRIPT_DIR}/assets/mime/pulsar-mime.xml" "${MIME_DIR}/pulsar-mime.xml"
        if command -v update-mime-database &> /dev/null; then
            update-mime-database "${HOME}/.local/share/mime" 2>/dev/null || true
        fi
    fi

    if command -v update-desktop-database &> /dev/null; then
        update-desktop-database "${APPS_DIR}" 2>/dev/null || true
    fi
fi

echo "=================================================="
echo "Pulsar installation complete!"
echo "Target binary: ${TARGET_BIN}"
echo "You can now run:"
echo "  pulsar         # Open unified desktop GUI"
echo "  pulsar --help  # Show CLI commands"
echo "  fs-hub         # Backward-compatible alias"
echo "Desktop launcher and .funscript file associations installed!"
echo "=================================================="
