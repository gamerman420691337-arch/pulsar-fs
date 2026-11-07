#!/usr/bin/env bash
# Qualified HSSP playback: the full Engine-controlled chain, end to end.
#
# Unlike bench-play.sh (which talks to the device API directly), every step here
# goes through the Engine's qualified path: accepted dossier, setup journal,
# controller lease, native device clock and the play journal. This is the
# product path; bench-play.sh is the bypass.
#
# Verified 2026-11-04 against Handy 2 Pro 4.3.0+dcee7e58: setup acknowledged,
# clock synchronized, combined playback clock acquired and a 201-point script
# played to completion through this chain.
#
# Two orderings are easy to get wrong and are load-bearing:
#   1. clock-sync RETIRES its controller, so a fresh playback preparation and a
#      fresh controller acquisition follow it before the combined clock is taken.
#   2. `handy-playback-clock` takes the *sync record* from clock-sync and the
#      *controller* from the second acquisition. Play then names the combined
#      clock request, not the sync request.
#
# Safety: attended only; the power switch is the stop. A stop command, a 200
# response or a frozen reported position is not proof of physical stopping.
#
# Usage:
#   ./scripts/bench-qualified-play.sh            # full chain, then play
#   ./scripts/bench-qualified-play.sh --stop-before-play   # everything but play
#
# Requires the Engine to be running with HANDY_API_KEY / HANDY_CONNECTION_KEY in
# its own environment; this script reads them from that process, by name only.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
P="$ROOT/target/debug/pulsar"
PROJ="d62d37a7-3064-492d-adad-ef84581b0515"
SCRIPT_SHA="901d197a47f69f72fd2beaf4916fb25279222ca9e1f6000bace67d6756f8c4a4"
BINDING="b0ab4dee541bb28d9efa1a6802296b8a96656a5a1acb686632c894cbcc9e86c9"
WORK="${WORK:-$HOME/.local/share/pulsar-qualification}"
STATE="${PULSAR_STATE_DIR:-$HOME/.local/state/pulsar}"
RUN="qp-$(date +%s)"
STOP_BEFORE_PLAY=0
[ "${1:-}" = "--stop-before-play" ] && STOP_BEFORE_PLAY=1

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
step() { printf '\n== %s\n' "$*"; }

[ -x "$P" ] || die "no executable at $P"

# Credentials live only in the Engine's environment.
SRC=""
for pid in $(ps -eo pid= | tr -d ' '); do
  [ -r "/proc/$pid/environ" ] 2>/dev/null || continue
  compat=$(tr '\0' '\n' 2>/dev/null < "/proc/$pid/environ" | grep -c '^HANDY_API_KEY=' || true)
  [ "$compat" = 1 ] && { SRC="$pid"; break; }
done
[ -n "$SRC" ] || die "no process holds HANDY_API_KEY; start the Engine with credentials"
API=$(tr '\0' '\n' < "/proc/$SRC/environ" | sed -n 's/^HANDY_API_KEY=//p')
CONN=$(tr '\0' '\n' < "/proc/$SRC/environ" | sed -n 's/^HANDY_CONNECTION_KEY=//p')
export HANDY_API_KEY="$API" HANDY_CONNECTION_KEY="$CONN"

API_BASE="https://www.handyfeeling.com/api/handy-rest/v3"
TOKEN=$(curl -sS -H "X-Api-Key: $API" "$API_BASE/auth/token/issue?to=$CONN&ttl=300" \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"]["token"])')

step "Device user offset (the dossier must declare it)"
OFFSET=$(curl -sS -H "Authorization: Bearer $TOKEN" -H "X-Connection-Key: $CONN" \
  "$API_BASE/hstp/offset" | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"]["offset"])')
export PULSAR_USER_OFFSET_MS="$OFFSET"
echo "   ${OFFSET} ms"

step "Clear unclaimed setups from earlier runs"
python3 - "$PROJ" <<'PY' > /tmp/qp-stale.txt
import sqlite3, json, os
state = os.environ.get("PULSAR_STATE_DIR", os.path.expanduser("~/.local/state/pulsar"))
con = sqlite3.connect(f"file:{state}/device-effects/journal.sqlite3?mode=ro", uri=True)
for i, rec in con.execute("SELECT id,record FROM setup_effects WHERE state='prepared'"):
    print(i, json.loads(rec)['request_id'])
PY
while read -r RID REQ; do
  [ -z "${RID:-}" ] && continue
  "$P" handy-cancel-setup "$PROJ" --setup-request-id "$REQ" --record-id "$RID" \
    --request-id "cx-$RUN-$REQ" >/dev/null 2>&1 && echo "   cancelled $REQ"
done < /tmp/qp-stale.txt

step "Re-author the dossier against the running image"
BINDING_SHA256="$BINDING" "$ROOT/scripts/refresh-handy-dossier.sh" >"/tmp/qp-author-$RUN.txt" 2>&1 \
  || { tail -3 "/tmp/qp-author-$RUN.txt"; die "authoring failed"; }
MANIFEST=$(grep "manifest sha256:" "/tmp/qp-author-$RUN.txt" | tail -1 | awk '{print $3}')
EPOCH=$(grep -m1 "engine epoch:" "/tmp/qp-author-$RUN.txt" | awk '{print $3}')
SESS=$(grep -m1 "device session:" "/tmp/qp-author-$RUN.txt" | awk '{print $3}')
[ -n "$MANIFEST" ] || die "authoring produced no manifest"
echo "   epoch $EPOCH"

step "Qualify (import + accept, attended bench mode)"
"$P" handy-qualify "$WORK/qualification-input.json" --manifest-sha256 "$MANIFEST" \
  --engine-epoch "$EPOCH" --device-session "$SESS" --note "bench $RUN" \
  --request-id "q-$RUN" --bench-mode >"/tmp/qp-q-$RUN.json" 2>&1 \
  || die "qualify: $(head -1 "/tmp/qp-q-$RUN.json")"
DOSSIER=$(python3 -c "import json;print(json.load(open('/tmp/qp-q-$RUN.json'))['value']['report']['dossier_id'])")
REV=$(python3 -c "import json;print(json.load(open('/tmp/qp-q-$RUN.json'))['value']['report']['revision'])")
echo "   dossier ${DOSSIER:0:16} rev $REV"

step "Host the script"
"$P" handy-host-stroke "$PROJ" --revision 1 --preparation-request-id prep-010 \
  --script-sha256 "$SCRIPT_SHA" --engine-epoch "$EPOCH" --request-id "h-$RUN" \
  --allow-temporary-public-hosting >"/tmp/qp-h-$RUN.json" 2>&1 \
  || die "hosting: $(head -1 "/tmp/qp-h-$RUN.json")"

step "Setup (loads the script, no motion)"
"$P" handy-setup "$PROJ" --revision 1 --engine-epoch "$EPOCH" --device-session "$SESS" \
  --hosting-request-id "h-$RUN" --qualification-id "$DOSSIER" --qualification-revision "$REV" \
  --request-id "s-$RUN" --allow-online-setup >"/tmp/qp-s-$RUN.json" 2>&1 \
  || die "setup: $(head -1 "/tmp/qp-s-$RUN.json")"
RECORD=$(python3 -c "import json;print(json.load(open('/tmp/qp-s-$RUN.json'))['value']['history']['record_id'])")
STATE_S=$(python3 -c "import json;print(json.load(open('/tmp/qp-s-$RUN.json'))['value']['history']['state'])")
echo "   $STATE_S"

prepare_and_acquire() {
  local tag=$1
  "$P" handy-prepare-playback "$PROJ" --setup-request-id "s-$RUN" --record-id "$RECORD" \
    --qualification-id "$DOSSIER" --qualification-revision "$REV" --revision 1 \
    --engine-epoch "$EPOCH" --device-session "$SESS" --hosting-request-id "h-$RUN" \
    --request-id "p$tag-$RUN" --allow-online-inspection >"/tmp/qp-p$tag-$RUN.json" 2>&1 \
    || die "prepare($tag): $(head -1 "/tmp/qp-p$tag-$RUN.json")"
  local prep
  prep=$(python3 -c "import json;print(json.load(open('/tmp/qp-p$tag-$RUN.json'))['value']['preparation_id'])")
  "$P" handy-controller-acquire "$PROJ" --revision 1 --preparation-request-id "p$tag-$RUN" \
    --preparation-id "$prep" --lease-ms 30000 --request-id "a$tag-$RUN" \
    --allow-arm --allow-online-inspection >"/tmp/qp-a$tag-$RUN.json" 2>&1 \
    || die "acquire($tag): $(head -1 "/tmp/qp-a$tag-$RUN.json")"
  python3 -c "import json;v=json.load(open('/tmp/qp-a$tag-$RUN.json'))['value']['lease'];print(v['controller_id'],v['generation'])"
}

step "Prepare + acquire the sync controller"
read -r C1 G1 <<<"$(prepare_and_acquire 1)"

step "Clock sync (this retires its controller by design)"
"$P" handy-clock-sync "$PROJ" --revision 1 --acquisition-request-id "a1-$RUN" \
  --controller-id "$C1" --generation "$G1" --request-id "c-$RUN" \
  --allow-online-synchronization >"/tmp/qp-c-$RUN.json" 2>&1 \
  || die "clock-sync: $(head -1 "/tmp/qp-c-$RUN.json")"
SYNC_RECORD=$(python3 -c "import json;print(json.load(open('/tmp/qp-c-$RUN.json'))['value']['history']['record_id'])")

step "Prepare + acquire the playing controller"
read -r C2 G2 <<<"$(prepare_and_acquire 2)"

step "Combined playback clock"
"$P" handy-playback-clock "$PROJ" --revision 1 --acquisition-request-id "a2-$RUN" \
  --controller-id "$C2" --generation "$G2" --sync-request-id "c-$RUN" \
  --sync-record-id "$SYNC_RECORD" --request-id "k-$RUN" --allow-online-timing \
  >"/tmp/qp-k-$RUN.json" 2>&1 || die "playback-clock: $(head -1 "/tmp/qp-k-$RUN.json")"

if [ "$STOP_BEFORE_PLAY" = 1 ]; then
  printf '\n== Ready to play. Re-run without --stop-before-play, or issue:\n'
  printf '   %s handy-play %s --revision 1 --acquisition-request-id a2-%s \\\n' "$P" "$PROJ" "$RUN"
  printf '     --controller-id %s --generation %s --clock-request-id k-%s \\\n' "$C2" "$G2" "$RUN"
  printf '     --start-time-ms 0 --request-id y-%s --allow-play\n' "$RUN"
  exit 0
fi

step "PLAY (motion)"
"$P" handy-play "$PROJ" --revision 1 --acquisition-request-id "a2-$RUN" \
  --controller-id "$C2" --generation "$G2" --clock-request-id "k-$RUN" \
  --start-time-ms 0 --request-id "y-$RUN" --allow-play >"/tmp/qp-y-$RUN.json" 2>&1 \
  || die "play: $(head -1 "/tmp/qp-y-$RUN.json")"
echo "   play issued; verify by observation. The power switch is the stop."
