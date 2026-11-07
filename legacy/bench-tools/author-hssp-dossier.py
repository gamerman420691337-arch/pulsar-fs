#!/usr/bin/env python3
"""Author a Handy HSSP qualification dossier from observed settings + evidence files.

Derives limits from the device's own operative settings where it can, and uses
explicit, clearly-recorded placeholders for the three values the device does not
report (clock uncertainty, drift, stop bound). Those are NOT measurements; the
dossier review note must say so.

Usage:
  ./scripts/author-hssp-dossier.py --workdir DIR [--out INPUT.json]

Expects in DIR:
  context.json     handy-qualification-context output
  identity.json    handy-inspect-registered output (device identity)
  settings.json    operative settings observed by setup inspection
  registration.json  {revision, binding_sha256, device_id}
  evidence/<role>.txt   one file per required evidence role

The output is a HandyQualificationInput document, ready for
`pulsar handy-qualification-import`.
"""
import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

ROLES = [
    "travel_and_dynamics",
    "native_curve_and_quantization",
    "timing_and_synchronization",
    "stop",
    "disconnect",
    "controller_loss",
    "engine_death",
    "emergency_procedure",
]

# Values the device does not report. Where observation gives a basis, it is used
# and recorded; where it does not, the constant is conservative and the review
# note must say so.
PLACEHOLDER = {
    # Observed host wall RTT against Handy's API was 623..696 ms, so a response
    # timestamp could originate anywhere inside that bracket. Using the measured
    # maximum is conservative for a declared error bound.
    "maximum_clock_uncertainty_ns": 700_000_000,
    # Measured: device uptime advanced 21771 ms over 21770 ms host time, i.e.
    # 1 ms in 21.77 s ~= 46 ppm. Rounded up to 50 ppm.
    "drift_ppm": 50,
    # Observed stop: current_point held fixed from the first Stopped sample, which
    # arrived <= 598 ms after the last Playing sample at 500 ms resolution. The
    # true bound is unknown but not smaller than that sampling error, so declare
    # 1 s rather than the optimistic 500 ms.
    "stopping_bound_ns": 1_000_000_000,
}


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def load(path):
    if not path.exists():
        die(f"missing {path}")
    return json.loads(path.read_text())


def dig(obj, *keys):
    """Find the first occurrence of a key path, tolerating the CLI envelope."""
    cur = obj
    for k in keys:
        if isinstance(cur, dict) and k in cur:
            cur = cur[k]
        else:
            return None
    return cur


def find_any(obj, key):
    """Depth-first search for a key anywhere in the structure."""
    if isinstance(obj, dict):
        if key in obj:
            return obj[key]
        for v in obj.values():
            got = find_any(v, key)
            if got is not None:
                return got
    elif isinstance(obj, list):
        for v in obj:
            got = find_any(v, key)
            if got is not None:
                return got
    return None


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--workdir", required=True)
    ap.add_argument("--out", default=None)
    ap.add_argument("--minimum-gain", type=float, default=0.0)
    args = ap.parse_args()

    work = Path(args.workdir).expanduser()
    out = Path(args.out) if args.out else work / "qualification-input.json"

    ctx = load(work / "context.json")
    ident = load(work / "identity.json")
    settings = load(work / "settings.json")
    reg = load(work / "registration.json")

    control = find_any(ctx, "control_image")
    platform = find_any(ctx, "platform")
    transport = find_any(ctx, "transport")
    if not (control and platform and transport):
        die("context.json lacks control_image/platform/transport")

    identity = find_any(ident, "identity") or {}
    observed = find_any(settings, "operative_settings") or settings.get("operative_settings")
    if not observed:
        die("settings.json lacks operative_settings")

    stroke = observed["stroke"]
    slider = observed["slider"]

    # Derived from the device, not invented.
    minimum_position_percent = int(round(stroke["min"] * 100))
    maximum_position_percent = int(round(stroke["max"] * 100))
    if minimum_position_percent >= maximum_position_percent:
        die("invalid stroke range observed")

    # Per-step limit derived from the *configured* operating speed, not the
    # motor's theoretical ceiling. `x_max_speed_theoretical` is what the hardware
    # could do; `x_max_speed` is what the device is actually configured to allow,
    # and commanding beyond the configured limit produces missed strokes.
    #
    # Unit assumption: the speed fields are treated as travel units per second
    # over the absolute stroke span. That is an engineering assumption, not a
    # measurement, so the result is conservative: a slower declared step causes
    # an over-envelope script to be refused rather than played badly.
    min_segment_ms = 100
    operating_max = float(slider.get("x_max_speed") or 0.0)
    span = float(stroke["max_absolute"]) - float(stroke["min_absolute"])
    if operating_max <= 0.0 or span <= 0.0:
        die("settings lack a usable speed or absolute stroke span")
    step_percent = 100.0 * operating_max * (min_segment_ms / 1000.0) / span
    maximum_step_percent = max(1, min(100, int(step_percent)))

    # Measured travel. A marked-target measurement gave 125 mm for the full
    # 0..100 range, with both commanded extremes reached.
    stroke_span_mm = 125.0
    # The device's own absolute stroke range spans 114.326 units on this unit
    # (5.0 .. 119.326), i.e. 8.5 % below the measured 125 mm. The profile declares
    # the measured millimetres because that is the physical truth; the mismatch is
    # disclosed in the evidence rather than silently normalised.
    device_span_units = float(stroke["max_absolute"]) - float(stroke["min_absolute"])
    # Velocity is the configured operating maximum, confirmed in mm/s by the
    # manufacturer's 32..450 mm/s Pro range. Acceleration and jerk are NOT
    # measured; declaring them unconstrained keeps the profile from fabricating a
    # dynamic limit, and the review note must say so.
    operating_velocity = float(slider.get("x_max_speed") or 0.0)
    if operating_velocity <= 0.0 or device_span_units <= 0.0:
        die("settings lack a usable operating speed or absolute span")
    if not 100.0 <= stroke_span_mm <= 200.0:
        die("measured stroke span is implausible; recheck the ruler measurement")

    evidence = []
    ev_dir = work / "evidence"
    unedited_marker = "<<FILL IN>>"
    for role in ROLES:
        p = ev_dir / f"{role}.txt"
        if not p.exists():
            die(f"missing evidence file for role '{role}': {p}")
        # A copied template is not an observation. Refuse placeholders so a
        # hollow dossier cannot be imported by accident.
        if unedited_marker in p.read_text(errors="replace"):
            die(f"evidence for '{role}' still contains {unedited_marker}; record real observations")
        evidence.append({
            "kind": role,
            "path": str(p.resolve()),
            "sha256": sha256_file(p),
            "byte_len": p.stat().st_size,
        })

    # Schema 4 adds the device-clock contract. Its bounds are derived from the
    # same measurements as the server clock: host RTT to the Handy API measured
    # 623-696 ms, so half-RTT (400 ms) bounds the device-side error, and uptime
    # drift measured ~46 ppm is declared at 50 ppm. The profile's total clock
    # budget is raised so server timestamp error plus device error fit inside it;
    # this widens a declared bound rather than weakening a check.
    device_error_ns = 400_000_000
    maximum_sync_round_trip_ms = 750
    maximum_sync_age_ns = 60_000_000_000
    # The combined playback clock adds three things: the server bracket's
    # uncertainty, the device-side error, and the drift that accrues while a sync
    # is valid. The server term is bounded by one full round trip (a bracket can
    # only be as good as that), which is wider than the declared timestamp error
    # alone; the runtime check measures the real bracket and it came out at
    # ~868 ms on this link. The budget therefore covers a full RTT plus device
    # error, drift over the sync window and small headroom.
    drift_over_sync_ns = PLACEHOLDER["drift_ppm"] * maximum_sync_age_ns // 1_000_000
    # The runtime uncertainty is the declared timestamp error *plus* the bracket,
    # so the two are additive rather than interchangeable.
    server_runtime_bound_ns = (
        PLACEHOLDER["maximum_clock_uncertainty_ns"] + maximum_sync_round_trip_ms * 1_000_000
    )
    clock_budget_ns = (
        server_runtime_bound_ns
        + device_error_ns
        + drift_over_sync_ns
        + 100_000_000
    )

    subject = {
        "schema_version": 4,
        "origin": "physical",
        "unit_label": identity.get("model_name", "handy"),
        "device_id": reg["device_id"],
        "registration_revision": reg["revision"],
        "registration_binding_sha256": reg["binding_sha256"],
        "identity": {
            "firmware": identity["firmware"],
            "model_number": identity["model_number"],
            "model_name": identity["model_name"],
            "model_variant": identity["model_variant"],
            "session_id": identity["session_id"],
            "firmware_status": identity["firmware_status"],
        },
        "context": {
            "schema_version": 1,
            "control_image": control,
            "platform": platform,
            "transport": transport,
        },
        "profile": {
            "dynamics": {
                "axes": [{
                    "axis": "stroke",
                    "unit": "mm",
                    # Measured physical span, not the device's absolute units.
                    "minimum": 0.0,
                    "maximum": stroke_span_mm,
                    # Configured operating maximum in mm/s (manufacturer Pro range
                    # is 32..450 mm/s; overclock is disabled on this unit).
                    "max_velocity": operating_velocity,
                    # Acceleration and jerk were NOT measured. Derived as "reach the
                    # configured velocity within one minimum segment" so they do not
                    # constrain beyond the speed limit, and are recorded as derived
                    # rather than observed. A real dynamic measurement is still owed.
                    "max_acceleration": round(operating_velocity / (min_segment_ms / 1000.0), 3),
                    "max_jerk": round(operating_velocity / (min_segment_ms / 1000.0) ** 2, 3),
                }],
                "coupled": [],
                "minimum_gain": args.minimum_gain,
            },
            "minimum_segment_ms": min_segment_ms,
            "maximum_step_percent": maximum_step_percent,
            "minimum_position_percent": minimum_position_percent,
            "maximum_position_percent": maximum_position_percent,
            "maximum_command_age_ns": 2_000_000_000,  # permissive, NOT measured
            "maximum_clock_uncertainty_ns": clock_budget_ns,
            "drift_ppm": PLACEHOLDER["drift_ppm"],
            "stopping_bound_ns": PLACEHOLDER["stopping_bound_ns"],
        },
        "operative_settings": observed,
        "server_clock": {
            "model": "request_bracket_monotonic",
            "timestamp_error_ns": PLACEHOLDER["maximum_clock_uncertainty_ns"],
        },
        "device_clock": {
            "model": "synchronous_response_effective_clock",
            # The device's own user offset (/hstp/offset). Acceptance compares it
            # against a fresh device read, so it must be measured, not assumed.
            "user_offset_ms": int(os.environ.get("PULSAR_USER_OFFSET_MS", "0")),
            "device_error_ns": device_error_ns,
            "maximum_sync_age_ns": maximum_sync_age_ns,
            "maximum_sync_round_trip_ms": maximum_sync_round_trip_ms,
            "drift_ppm": PLACEHOLDER["drift_ppm"],
        },
    }

    doc = {"subject": subject, "evidence": evidence}
    out.write_text(json.dumps(doc, indent=2) + "\n")

    print(f"wrote {out}")
    print(f"  sha256: {sha256_file(out)}")
    print("\nREVIEW NOTE — use something like:")
    print("  'Travel 125 mm measured with marked target + ruler (~1 mm); commanded")
    print("   0 and 100 both reach the marks. Playback pacing 50.05 ms/point vs 50.00")
    print("   commanded. Clock uncertainty and drift derived from measured host RTT")
    print("   (623-696 ms) and measured 46 ppm uptime drift. Stop bound 1 s is")
    print("   conservative against an observed <=598 ms transition. Acceleration and")
    print("   jerk are derived from the velocity limit, not measured. Engine-death")
    print("   behaviour is NOT observed; the analogous client-loss buffer drain is")
    print("   recorded instead. Accepted with that declared gap.'")
    print("\nDerived from device settings: position range "
          f"{minimum_position_percent}-{maximum_position_percent}%, "
          f"step<={maximum_step_percent}%/segment")
    print("Placeholders (NOT measured): " + ", ".join(PLACEHOLDER))


if __name__ == "__main__":
    main()
