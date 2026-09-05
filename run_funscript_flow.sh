#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
VENV="$HERE/.venv"

if [[ ! -d "$VENV" ]]; then
    echo "Virtual environment not found at $VENV. Initializing with uv..."
    uv venv "$VENV" --python 3.10
    uv pip install --python "$VENV" opencv-python numpy decord scipy pytest
fi

exec "$VENV/bin/python" "$HERE/funscript_flow.py" "$@"
