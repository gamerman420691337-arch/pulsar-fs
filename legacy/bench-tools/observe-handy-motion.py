#!/usr/bin/env python3
"""Observe real Handy motion via REST and record it as evidence.

Pulsar cannot drive the device until a qualification dossier is accepted, but the
device's own state can be observed while another client (the Handy app) plays a
script. That observation is real physical evidence: position, velocity, direction
and play-state transitions sampled from the device itself.

Nothing here sends a motion command. It is read-only.

Credentials come from the environment:
    HANDY_API_KEY, HANDY_CONNECTION_KEY

Usage:
    ./scripts/observe-handy-motion.py --seconds 30
    ./scripts/observe-handy-motion.py --seconds 30 --out stop-run.json

Then have the Handy app play a script; start this first so the whole run is
captured, including the Stop transition.
"""
import argparse
import json
import os
import sys
import time
import urllib.request

ORIGIN = "https://www.handyfeeling.com/api/handy-rest/v3"


def die(m):
    print(f"error: {m}", file=sys.stderr)
    sys.exit(1)


class Client:
    def __init__(self):
        self.api = os.environ.get("HANDY_API_KEY")
        self.conn = os.environ.get("HANDY_CONNECTION_KEY")
        if not self.api or not self.conn:
            die("HANDY_API_KEY and HANDY_CONNECTION_KEY must be set")
        self.token = None
        self.renew()

    def _get(self, path, auth):
        req = urllib.request.Request(ORIGIN + path, headers={"Accept": "application/json", **auth})
        return json.load(urllib.request.urlopen(req, timeout=15))

    def renew(self):
        r = self._get(f"/auth/token/issue?to={self.conn}&ttl=3600", {"X-Api-Key": self.api})
        self.token = r["result"]["token"]

    def get(self, path):
        return self._get(path, {"Authorization": f"Bearer {self.token}",
                                "X-Connection-Key": self.conn})

    def renew_if_needed(self, started, life=3000):
        if time.time() - started > life:
            self.renew()


def sample(c, mode):
    """One observation. HAMP reports velocity; HSP/HSSP report progress.

    `mode` is passed in rather than re-read every tick: the device API allows
    240 requests/minute, so mode is sampled separately and cached.
    """
    now = time.time()
    out = {"host_ms": int(now * 1000), "mode": mode}
    if mode == 0:
        s = c.get("/hamp/state")["result"]
        # HAMP reports velocity/direction and the current program's bounds; it has
        # no absolute position field. Do not present bounds as a position.
        out.update(play_state=s["play_state"], velocity=s["velocity"],
                   direction=s["direction"],
                   hamp_bounds=[s["min"], s["max"]])
    elif mode in (1, 4):
        path = "/hssp/state" if mode == 1 else "/hsp/state"
        s = c.get(path)["result"]
        out.update(play_state=s["play_state"], current_time=s.get("current_time"),
                   current_point=s.get("current_point"), points=s.get("points"),
                   playback_rate=s.get("playback_rate"))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seconds", type=float, default=30)
    ap.add_argument("--hz", type=float, default=2)
    ap.add_argument("--out", default="-")
    args = ap.parse_args()

    c = Client()
    started = time.time()
    rows = []
    errors = 0
    period = 1.0 / args.hz
    # The API allows 240 requests/minute (4/s). At 2 Hz with one state call per
    # tick plus a mode re-read every 5 s we stay near 2.2/s.
    mode = None
    mode_read = 0.0
    while time.time() - started < args.seconds:
        tick = time.time()
        try:
            c.renew_if_needed(started)
            if mode is None or tick - mode_read > 5.0:
                mode = c.get("/mode")["result"]["mode"]
                mode_read = tick
            rows.append(sample(c, mode))
        except Exception as e:  # noqa: BLE001 - any transport failure is a gap, not a stop
            errors += 1
            rows.append({"host_ms": int(tick * 1000), "error": type(e).__name__})
        time.sleep(max(0.0, period - (time.time() - tick)))

    good = [r for r in rows if "error" not in r]
    report = {
        "samples": len(rows),
        "errors": errors,
        "duration_s": round(time.time() - started, 2),
        "modes_seen": sorted({r["mode"] for r in good}),
        "play_states": sorted({r.get("play_state") for r in good if "play_state" in r}),
        "rows": rows,
    }
    if good and "velocity" in good[0]:
        v = [r["velocity"] for r in good if "velocity" in r]
        report["velocity_range"] = [min(v), max(v)]
        dirs = {r["direction"] for r in good if "direction" in r}
        report["directions_seen"] = sorted(dirs)
    if good and "current_time" in good[0]:
        t = [r["current_time"] for r in good if "current_time" in r]
        p = [r["current_point"] for r in good if "current_point" in r]
        report["current_time_range"] = [min(t), max(t)]
        report["current_point_range"] = [min(p), max(p)]
    print(json.dumps({k: v for k, v in report.items() if k != "rows"}, indent=2))
    text = json.dumps(report, indent=1)
    if args.out == "-":
        print("\n(rows omitted; use --out FILE to keep them)")
    else:
        with open(args.out, "w") as f:
            f.write(text + "\n")
        print(f"\nwrote {args.out}")


if __name__ == "__main__":
    main()
