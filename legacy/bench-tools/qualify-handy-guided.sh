#!/usr/bin/env bash
# Guided Handy 2 Pro HSSP qualification — one sitting, no hand-authored JSON.
#
# Produces a real accepted dossier so the desktop can arm playback.
#
# IMPORTANT: the dossier binds the control-image hash of the *current* binary.
# Rebuilding pulsar afterwards invalidates it. Finish qualification, then do not
# rebuild before testing.
#
# Never pass the connection key as an argument. Export it in the environment
# that launches the Engine:
#     export HANDY_CONNECTION_KEY=...        # or: source ~/.config/pulsar/handy.env
#
set -euo pipefail

PULSAR="${PULSAR:-./target/debug/pulsar}"
DEVICE="${DEVICE:-handy-main}"
LABEL="${LABEL:-Handy 2 Pro}"
PROJECT="${PROJECT:-}"
REVISION="${REVISION:-}"
WORK="${WORK:-$HOME/.local/share/pulsar-qualification}"
STAMP="$(date +%Y%m%d-%H%M%S)"

say() { printf '\n=== %s ===\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

[ -x "$PULSAR" ] || die "pulsar not found at $PULSAR (set PULSAR=)"
# The Engine, not this shell, must hold the credential: it resolves the variable
# named in the registration. Only warn when a local variable is absent.
if [ -z "${HANDY_CONNECTION_KEY:-}" ]; then
  printf 'note: HANDY_CONNECTION_KEY is not set in this shell. The Engine must have it;\n'
  printf '      inspection will refuse with a missing-credential error otherwise.\n'
fi
mkdir -p "$WORK"

say "0. Qualification context (binds this exact binary)"
CONTEXT="$("$PULSAR" handy-qualification-context --transport hssp)"
printf '%s\n' "$CONTEXT" > "$WORK/context-$STAMP.json"
printf '%s\n' "$CONTEXT"

say "1. Register credential references (values never appear here)"
"$PULSAR" handy-register "$DEVICE" \
  --label "$LABEL" \
  --api-key-env HANDY_API_KEY \
  --connection-key-env HANDY_CONNECTION_KEY \
  --request-id "qual-register-$STAMP" || true

say "2. Registrations"
"$PULSAR" handy-registrations

say "3. Read-only identity inspection (no motion)"
"$PULSAR" handy-inspect-registered "$DEVICE" --allow-online-inspection \
  | tee "$WORK/identity-$STAMP.json" \
  || die "inspection failed. Check the Engine inherited HANDY_CONNECTION_KEY,
        and that the device is online."

say "NEXT STEPS (manual — these need a project and a connected session)"
cat <<'EOF'

Registration and identity are done. The remaining steps need a hosted script
and an open connection session, which depend on a specific project:

  a) Open a connection session and record ENGINE_EPOCH + DEVICE_SESSION:
       pulsar handy-open-session ...

  b) Prepare and host a short Stroke script from a committed project:
       pulsar handy-prepare-stroke PROJECT out.csv --revision REV --request-id ...
       pulsar handy-host-stroke PROJECT --revision REV \
         --preparation-request-id ... --script-sha256 ... \
         --engine-epoch ... --request-id ... --allow-temporary-public-hosting

  c) Read the live operative settings via setup inspection:
       pulsar handy-inspect-setup PROJECT --revision REV \
         --engine-epoch ... --device-session ... --hosting-request-id ... \
         --request-id ... --allow-online-inspection

  d) Author evidence for the eight roles (see evidence/README.md in this work dir),
     then run the dossier builder:
       ./scripts/author-hssp-dossier.py --workdir WORK

  e) Import, review, accept:
       pulsar handy-qualification-import INPUT.json --manifest-sha256 SHA --request-id ...
       pulsar handy-qualification-status DOSSIER_ID
       pulsar handy-qualification-accept DOSSIER_ID --revision R --engine-epoch E \
         --device-session S --note 'method: marked target + ruler; resolution ~200ms' \
         --request-id ... --acknowledge-physical-evidence --allow-online-inspection

EOF

printf 'Work directory: %s\n' "$WORK"
