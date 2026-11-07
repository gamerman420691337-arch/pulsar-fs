#!/usr/bin/env bash
# Bench playback: load a hosted script and play it on the Handy directly.
#
# This is a BENCH tool, not the product path. It talks to the Handy REST v3
# device API directly and does not go through the Engine's qualified chain
# (dossier / journal / controller lease / clock sync). Use it to prove motion,
# not to claim qualification.
#
# Safety: attended only. The power switch is the stop. A stop command, a 200
# response or a frozen current_point is not proof of physical stopping.
#
# Credentials come from the Engine's environment, by name only:
#   HANDY_API_KEY, HANDY_CONNECTION_KEY
#
# Usage:
#   ./scripts/bench-play.sh                  # play the default script once
#   ./scripts/bench-play.sh --stop           # stop only
#   SCRIPT_URL=... ./scripts/bench-play.sh   # play a different hosted CSV
#   ./scripts/bench-play.sh --dry-run        # report state, change nothing
set -euo pipefail

API_BASE="https://www.handyfeeling.com/api/handy-rest/v3"
SCRIPT_URL="${SCRIPT_URL:-https://www.handyfeeling.com/api/hosting/v2/download/b88addcb6a14b2ddf4975be13c99989cfd4133ab8b4369ffc4f7f99352e6fda0}"
WATCH_SECONDS="${WATCH_SECONDS:-24}"

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# Read credentials from the running Engine's environment (never argv, never a file).
engine_pid() {
  local pid
  for pid in $(ps -eo pid= | tr -d ' '); do
    [ -r "/proc/$pid/environ" ] 2>/dev/null || continue
    if tr '\0' '\n' 2>/dev/null < "/proc/$pid/environ" | grep -q '^HANDY_API_KEY='; then
      if tr '\0' '\n' < "/proc/$pid/cmdline" 2>/dev/null | sed -n '2p' | grep -qx 'engine'; then
        echo "$pid"; return 0
      fi
    fi
  done
  for pid in $(ps -eo pid= | tr -d ' '); do
    [ -r "/proc/$pid/environ" ] 2>/dev/null || continue
    tr '\0' '\n' 2>/dev/null < "/proc/$pid/environ" | grep -q '^HANDY_API_KEY=' && { echo "$pid"; return 0; }
  done
  return 0
}

PID="$(engine_pid)"
[ -n "$PID" ] || die "no process holds HANDY_API_KEY; start the Engine with credentials first"
API_KEY="$(tr '\0' '\n' < "/proc/$PID/environ" | sed -n 's/^HANDY_API_KEY=//p')"
CONN_KEY="$(tr '\0' '\n' < "/proc/$PID/environ" | sed -n 's/^HANDY_CONNECTION_KEY=//p')"
[ -n "$API_KEY" ] && [ -n "$CONN_KEY" ] || die "credentials are not both present in pid $PID"

TOKEN="$(curl -sS -H "X-Api-Key: $API_KEY" \
  "$API_BASE/auth/token/issue?to=$CONN_KEY&ttl=900" \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"]["token"])')"
[ -n "$TOKEN" ] || die "token issuance failed"

H=(-H "Authorization: Bearer $TOKEN" -H "X-Connection-Key: $CONN_KEY")
state() { curl -sS "${H[@]}" "$API_BASE/hssp/state"; }

if [ "${1:-}" = "--stop" ]; then
  curl -sS -o /dev/null -X PUT "${H[@]}" -H "Content-Type: application/json" \
    -d '{"start_time":0,"server_time":0,"playback_rate":1.0,"loop":false}' "$API_BASE/hssp/stop"
  echo "stop requested (not proof of physical stopping)"
  exit 0
fi

if [ "${1:-}" = "--dry-run" ]; then
  echo "connected: $(curl -sS "${H[@]}" "$API_BASE/connected" | tr -d '\n ')"
  echo "mode     : $(curl -sS "${H[@]}" "$API_BASE/mode" | tr -d '\n ')"
  state | python3 -c 'import json,sys;r=json.load(sys.stdin)["result"];print("play_state=%s points=%s last_point_time=%s" % (r["play_state"], r["points"], r["last_point_time"]))'
  exit 0
fi

echo "1) HSSP mode"
curl -sS -o /dev/null -X PUT "${H[@]}" -H "Content-Type: application/json" \
  -d '{"mode":1}' "$API_BASE/mode"

echo "2) load script (no motion)"
curl -sS -o /dev/null -X PUT "${H[@]}" -H "Content-Type: application/json" \
  -d "{\"url\":\"$SCRIPT_URL\",\"loop\":false,\"notify\":false}" "$API_BASE/hssp/setup"
state | python3 -c 'import json,sys;r=json.load(sys.stdin)["result"];print("   points=%s last_point_time=%s" % (r["points"], r["last_point_time"]))'

echo "3) PLAY -- motion begins"
NOW="$(python3 -c 'import time;print(int(time.time()*1000))')"
curl -sS -o /dev/null -X PUT "${H[@]}" -H "Content-Type: application/json" \
  -d "{\"start_time\":0,\"server_time\":$NOW,\"playback_rate\":1.0,\"loop\":false}" "$API_BASE/hssp/play"

echo "4) watching ${WATCH_SECONDS}s"
for _ in $(seq 1 "$WATCH_SECONDS"); do
  state | python3 -c 'import json,sys;r=json.load(sys.stdin)["result"];print("   play_state=%s current_time=%s point=%s" % (r["play_state"], r["current_time"], r["current_point"]))'
  sleep 1
done

echo "5) stop requested (verify by observation; the power switch is the stop)"
curl -sS -o /dev/null -X PUT "${H[@]}" -H "Content-Type: application/json" \
  -d '{"start_time":0,"server_time":0,"playback_rate":1.0,"loop":false}' "$API_BASE/hssp/stop"
