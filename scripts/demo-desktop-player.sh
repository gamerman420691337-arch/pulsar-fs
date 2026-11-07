#!/usr/bin/env bash
# Local Desktop demo. Credentials stay in the encrypted secrets store/environment.
set -euo pipefail
root="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
demo_root="${PULSAR_DEMO_DIR:-/mnt/4tb/artifacts/Funscripts/expire/desktop-player-demo}"
for required in mpv secrets; do
  command -v "$required" >/dev/null || { echo "Missing $required" >&2; exit 1; }
done
if [[ $# == 0 ]]; then
  mkdir -p "$demo_root"
  video="$demo_root/demo.mp4"
  script="$demo_root/demo.funscript"
  if [[ ! -f "$video" ]]; then
    timeout 30 ffmpeg -hide_banner -loglevel error -f lavfi -i 'testsrc2=size=640x360:rate=30:duration=12' \
      -f lavfi -i 'sine=frequency=440:duration=12' -c:v mpeg4 -q:v 5 -c:a aac -shortest "$video"
  fi
  if [[ ! -f "$script" ]]; then
    python3 - "$script" <<'PY'
import json,sys
with open(sys.argv[1], 'x') as f:
    json.dump({'version':'1.0','inverted':False,'range':100,
               'actions':[{'at':t,'pos':20 if t//1000%2==0 else 80} for t in range(0,12001,1000)]},f)
PY
  fi
elif [[ $# == 2 ]]; then
  video="$(realpath -- "$1")"
  script="$(realpath -- "$2")"
else
  echo "Usage: $0 [VIDEO FUNSCRIPT]" >&2
  exit 1
fi
binary="${PULSAR_DESKTOP_BIN:-$root/target/debug/pulsar}"
if [[ ! -x "$binary" || ! -x "$(dirname "$binary")/pulsar-worker" ]]; then
  echo 'Build first:' >&2
  echo "env CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_OPT_LEVEL=0 priority-build cargo build --manifest-path \"$root/Cargo.toml\" -p pulsar -p pulsar-worker --locked" >&2
  exit 1
fi
umask 077
state="${PULSAR_DEMO_STATE_DIR:-$demo_root/state}"
mkdir -p "$state"
echo 'Open video + script, enable Handy, then Play. Stop is always available.'
exec secrets run -- env PULSAR_STATE_DIR="$state" PULSAR_DEMO_VIDEO="$video" PULSAR_DEMO_SCRIPT="$script" "$binary" gui
