#!/usr/bin/env bash
# Re-author the Handy qualification dossier against the current binary and device.
#
# Why this exists: a dossier binds the control-image hash of the running Engine
# binary and the device's observed identity/settings. Any rebuild changes the image
# hash, and any setting change invalidates the subject, so the dossier must be
# re-authored before acceptance. Repeated observations are not re-invented here --
# the evidence files are reused exactly as written.
#
# Order matters: build first, restart the Engine, then run this. Authoring against
# a stale Engine produces a dossier that cannot be accepted.
#
# Usage:
#   ./scripts/refresh-handy-dossier.sh
#   DRY_RUN=1 ./scripts/refresh-handy-dossier.sh    # show what would be captured
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PULSAR="${PULSAR:-$ROOT/target/debug/pulsar}"
WORK="${WORK:-$HOME/.local/share/pulsar-qualification}"
DEVICE="${DEVICE:-handy-main}"
REGISTRATION_REVISION="${REGISTRATION_REVISION:-1}"
BINDING="${BINDING_SHA256:-}"
DRY_RUN="${DRY_RUN:-0}"

say() { printf '\n=== %s ===\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[ -x "$PULSAR" ] || die "no executable at $PULSAR"
mkdir -p "$WORK"

say "Engine and device"
"$PULSAR" handy-sessions >/dev/null || die "Engine not reachable"

EPOCH="$("$PULSAR" handy-sessions | python3 -c 'import json,sys;print(json.load(sys.stdin)["value"]["engine_epoch"])')"
echo "engine epoch: $EPOCH"

# Reuse a current observation session when one already exists; the reservation is
# exclusive, so opening a second one fails rather than silently succeeding.
SESS="$("$PULSAR" handy-sessions | python3 -c '
import json,sys
v=json.load(sys.stdin)["value"]
for s in v["sessions"]:
    if s.get("state") == "observed":
        print(s["device_session"]); break
')"
if [ -z "$SESS" ]; then
  [ -n "$BINDING" ] || die "no observation session and BINDING_SHA256 is unset"
  SESS="$("$PULSAR" handy-open-session "$DEVICE" \
    --engine-epoch "$EPOCH" \
    --expected-registration-revision "$REGISTRATION_REVISION" \
    --binding-sha256 "$BINDING" \
    --request-id "refresh-open-$(date +%s)" \
    --allow-online-inspection \
    | python3 -c 'import json,sys;print(json.load(sys.stdin)["value"]["device_session"])')"
fi
echo "device session: $SESS"

if [ "$DRY_RUN" = "1" ]; then
  say "dry run"
  echo "would capture context, identity and registration into $WORK, then re-author."
  echo "settings.json is reused; acceptance compares it against the live device."
  exit 0
fi

say "Capture context, identity, registration"
"$PULSAR" handy-qualification-context --transport hssp > "$WORK/context.json"

"$PULSAR" handy-inspect-registered "$DEVICE" --allow-online-inspection > "$WORK/inspect-identity.json"
python3 - "$WORK" <<'PY'
import json, pathlib, sys
w = pathlib.Path(sys.argv[1])
d = json.load(open(w / "inspect-identity.json"))["value"]
json.dump({"identity": d["inspection"]["identity"]}, open(w / "identity.json", "w"), indent=1)
reg = d["registration"]
json.dump({"device_id": reg["device_id"], "revision": reg["revision"],
           "binding_sha256": reg["binding_sha256"]}, open(w / "registration.json", "w"), indent=1)
print("identity session_id:", d["inspection"]["identity"]["session_id"])
print("registration     :", reg["device_id"], "rev", reg["revision"])
PY

[ -f "$WORK/settings.json" ] || die "settings.json missing; run setup inspection once to capture operative settings"

say "Author dossier"
"$ROOT/scripts/author-hssp-dossier.py" --workdir "$WORK" | tail -6

SHA="$(python3 -c "import hashlib,sys;print(hashlib.sha256(open('$WORK/qualification-input.json','rb').read()).hexdigest())")"
printf '%s\n' "$SHA" > "$WORK/qualification-input.sha256"

say "Next"
cat <<EOF
manifest sha256: $SHA
engine epoch   : $EPOCH
device session : $SESS

  $PULSAR handy-qualification-import $WORK/qualification-input.json \\
    --manifest-sha256 $SHA --request-id qual-import-<n>

  # then take the dossier id from that reply, review the evidence, and accept:
  $PULSAR handy-qualification-accept <DOSSIER_ID> --revision 1 \\
    --engine-epoch $EPOCH --device-session $SESS \\
    --note '<your review basis>' --request-id qual-accept-<n> \\
    --acknowledge-physical-evidence --allow-online-inspection

Do not rebuild or restart the Engine between authoring and acceptance: both change
the control-image identity the dossier binds.
EOF
