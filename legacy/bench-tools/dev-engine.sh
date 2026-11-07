#!/usr/bin/env bash
# Restart the Pulsar Engine on the current binary, preserving credentials from
# the caller's environment.
#
# The Engine has no self-restart by design: a launching client must never kill or
# replace a running Engine, because it owns durable jobs and device custody. This
# script is an explicit operator action, not a client behaviour.
#
# Credentials are read from the environment and never stored, echoed or passed as
# arguments:
#   HANDY_API_KEY          Handy ApplicationKey (only used for /auth/token/issue)
#   HANDY_CONNECTION_KEY   Handy device connection key
#
# Usage:
#   ./scripts/dev-engine.sh              # restart (or start) the Engine
#   ./scripts/dev-engine.sh --stop       # graceful stop only
#   ./scripts/dev-engine.sh --status     # report state, change nothing
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${PULSAR_BIN:-$ROOT/target/debug/pulsar}"
STATE_DIR="${PULSAR_STATE_DIR:-$HOME/.local/state/pulsar}"
SOCKET="$STATE_DIR/engine.sock"
LOG="${PULSAR_ENGINE_LOG:-/tmp/pulsar-engine.log}"
WAIT_STEPS=100   # 100 x 100ms = 10s, matching the launcher's startup budget

cd "$ROOT"

engine_pid() {
  # The Engine is launched with a path that may be relative (./target/debug/pulsar)
  # or absolute. Match on the executable's resolved form as reported by /proc,
  # never on this script's own arguments.
  local pid exe
  for pid in $(ps -eo pid= | tr -d ' '); do
    [ -r "/proc/$pid/cmdline" ] || continue
    # Only processes whose argv is exactly `<pulsar> engine`.
    exe="$(tr '\0' '\n' < "/proc/$pid/cmdline" 2>/dev/null | sed -n '1p;2p' | paste -sd'|')"
    case "$exe" in
      *pulsar\|engine) ;;
      *) continue ;;
    esac
    # Confirm it is this build's executable path. Compare paths, not resolved
    # inodes: a running Engine keeps its original file after a rebuild, and a
    # stale binary is precisely what a restart needs to replace.
    local exe_path
    exe_path="$(readlink "/proc/$pid/exe" 2>/dev/null)"
    exe_path="${exe_path% (deleted)}"
    if [ "$exe_path" = "$(readlink -f "$BIN")" ]; then
      echo "$pid"
      return 0
    fi
  done
  return 0
}

status() {
  local pid
  pid="$(engine_pid)"
  if [ -z "$pid" ]; then
    echo "engine: not running"
    return 1
  fi
  echo "engine: pid $pid"
  echo "started: $(ps -o lstart= -p "$pid" | sed 's/^ *//')"
  # Report only whether the variables are present; never their values.
  local api conn
  api="$(tr '\0' '\n' < "/proc/$pid/environ" | grep -c '^HANDY_API_KEY=' || true)"
  conn="$(tr '\0' '\n' < "/proc/$pid/environ" | grep -c '^HANDY_CONNECTION_KEY=' || true)"
  echo "HANDY_API_KEY: $([ "$api" = 1 ] && echo present || echo ABSENT)"
  echo "HANDY_CONNECTION_KEY: $([ "$conn" = 1 ] && echo present || echo ABSENT)"
  echo "binary: $BIN"
  # A running Engine keeps its original image after a rebuild. Report this so a
  # stale Engine is not mistaken for a code change that did not take effect.
  local running
  running="$(readlink "/proc/$pid/exe" 2>/dev/null)"
  if [ -n "$running" ] && [ ! "$running" -ef "$BIN" ]; then
    echo "WARNING: Engine is running an older image than $BIN (rebuilt since start; restart to apply)"
  fi
  [ -S "$SOCKET" ] && echo "socket: $SOCKET" || echo "socket: MISSING"
}

stop_engine() {
  local pid
  pid="$(engine_pid)"
  if [ -z "$pid" ]; then
    echo "engine: not running"
    return 0
  fi
  echo "stopping engine pid $pid (graceful SIGTERM)"
  kill "$pid"
  local i
  for i in $(seq "$WAIT_STEPS"); do
    kill -0 "$pid" 2>/dev/null || { echo "engine: stopped"; return 0; }
    sleep 0.1
  done
  echo "ERROR: engine did not exit within 10s; refusing to start a second one" >&2
  echo "inspect pid $pid before retrying" >&2
  return 1
}

start_engine() {
  : "${HANDY_API_KEY:?HANDY_API_KEY is not set in this shell}"
  : "${HANDY_CONNECTION_KEY:?HANDY_CONNECTION_KEY is not set in this shell}"
  [ -x "$BIN" ] || { echo "ERROR: no executable at $BIN (build first)" >&2; return 1; }

  mkdir -p "$STATE_DIR"
  echo "starting $BIN engine (log: $LOG)"
  "$BIN" engine >"$LOG" 2>&1 &
  local i
  for i in $(seq "$WAIT_STEPS"); do
    if [ -S "$SOCKET" ]; then
      echo "engine: up, pid $(engine_pid)"
      return 0
    fi
    sleep 0.1
  done
  echo "ERROR: engine socket did not appear within 10s" >&2
  echo "--- last log lines ---" >&2
  tail -20 "$LOG" >&2 || true
  if grep -q "cgroup creation boot changed" "$LOG" 2>/dev/null; then
    cat >&2 <<'EOF'

The Linux boot changed since the Engine last ran, and automatic reboot recovery
is an unfinished product requirement. If the ledger holds no owned work, refresh
it explicitly:

    ./scripts/recover-cgroup-boot.py --dry-run
    ./scripts/recover-cgroup-boot.py
EOF
  fi
  return 1
}

case "${1:-restart}" in
  --status|status) status ;;
  --stop|stop)     stop_engine ;;
  restart|-r|"")   stop_engine && start_engine && echo && status ;;
  *)
    echo "usage: $0 [restart|--stop|--status]" >&2
    exit 2
    ;;
esac
