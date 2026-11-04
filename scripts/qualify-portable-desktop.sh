#!/usr/bin/env bash
set -euo pipefail

# Desktop package workflows are qualified through the same production client
# controller used by egui. This gate does not qualify native window rendering,
# other operating systems, detector accuracy, or physical devices.
if [[ "$(uname -s)" != "Linux" ]]; then
    printf '%s\n' 'Portable desktop process qualification is currently Linux-only; this host is not qualified.' >&2
    exit 2
fi

cd "$(dirname "$0")/.."
bash scripts/qualify-project-imports.sh
cargo test -p pulsar-clients portable:: -- --nocapture
cargo test --test portable_desktop_process -- --nocapture
