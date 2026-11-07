#!/usr/bin/env python3
"""Summarise an observe-handy-motion.py capture.

Reports what the device itself reported: play-state transitions, position or
progress range, reversal cadence, and the observed Stop latency.

Honesty rules applied here:
  * Stop latency is measured at the sampler's resolution, not the device's.
    With a 2 Hz sampler the bound is +/-250 ms and is reported as such.
  * A gap in samples is not a stop. Only an observed play_state transition is.
  * HAMP reports velocity/direction and program bounds, never absolute position;
    no position is invented for it.
"""
import argparse
import json
import sys

STOPPED, PLAYING = 0, 1
STATES = {0: "not_initialized", 1: "playing", 2: "stopped", 3: "paused", 4: "starving"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("capture")
    ap.add_argument("--hz", type=float, default=2.0, help="sampler rate used for the capture")
    args = ap.parse_args()

    d = json.load(open(args.capture))
    rows = d["rows"]
    good = [r for r in rows if "error" not in r]
    if not good:
        print("no usable samples", file=sys.stderr)
        return 1
    res_ms = 1000.0 / args.hz

    print(f"file        {args.capture}")
    print(f"samples     {len(rows)} ({len(rows)-len(good)} errors)")
    span = (good[-1]["host_ms"] - good[0]["host_ms"]) / 1000.0
    print(f"span        {span:.1f} s")
    print(f"modes       {sorted({r['mode'] for r in good})}")
    print(f"resolution  {res_ms:.0f} ms per sample (stop latency cannot be finer)")

    # Play-state transitions.
    print("\nplay-state transitions")
    prev = None
    transitions = []
    for r in good:
        st = r.get("play_state")
        if st is None:
            continue
        if prev is not None and st != prev[1]:
            dt = (r["host_ms"] - prev[0]) / 1000.0
            transitions.append((dt, prev[1], st, r))
            print(f"  {prev[1]} -> {st:<2} after {dt:6.2f} s")
        prev = (r["host_ms"], st)
    if not transitions:
        print("  (none observed — the run may not include a state change)")

    # Position / progress range.
    if "current_point" in good[0]:
        pts = [r["current_point"] for r in good if "current_point" in r]
        times = [r["current_time"] for r in good if "current_time" in r]
        print(f"\ncurrent_point range  {min(pts)} .. {max(pts)}")
        print(f"current_time  range  {min(times)} .. {max(times)} ms")
        print(f"points in program    {sorted({r['points'] for r in good if 'points' in r})}")
    elif "velocity" in good[0]:
        vel = [r["velocity"] for r in good if "velocity" in r]
        print(f"\nvelocity values      {sorted(set(vel))}")
        print("position             not reported by HAMP (velocity/direction only)")

    # Reversal cadence from direction flags (HAMP).
    if "direction" in good[0]:
        flips, prev = [], None
        for r in good:
            if "direction" not in r:
                continue
            if prev is not None and r["direction"] != prev["direction"]:
                flips.append(r["host_ms"] - prev["host_ms"])
            prev = r
        print(f"\ndirection reversals  {len(flips)}")
        if len(flips) > 1:
            # Drop doubled intervals caused by sampler aliasing (a missed flip).
            base = sorted(flips)[len(flips) // 2]
            clean = [f for f in flips if f < base * 1.5]
            mean = sum(clean) / len(clean)
            print(f"  half-period (clean) {mean:.0f} ms  n={len(clean)}")
            print(f"  implied full stroke {2*mean:.0f} ms  ({1000/(2*mean):.2f} Hz)")
            print(f"  dropped by aliasing {len(flips)-len(clean)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
