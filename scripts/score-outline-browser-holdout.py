#!/usr/bin/env python3
"""Score a Browser Live capture against generated frame-exact outline labels."""

import argparse
import json
from pathlib import Path
import runpy

import numpy as np


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recording", type=Path)
    parser.add_argument("model", type=Path)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("case")
    args = parser.parse_args()
    scorer = runpy.run_path(str(Path(__file__).with_name("score-sterile-browser-recording.py")))
    manifest = json.loads(args.manifest.read_text())
    if manifest["origin"] != "independently generated geometry" or manifest["playback_offset_ms"] != 0:
        parser.error("expected a generated zero-offset outline holdout")
    fps, frames = manifest["fps"], manifest["frames"]
    if not isinstance(fps, int) or not 1 <= fps <= 60 or not isinstance(frames, int) or not 2 <= frames <= 1000:
        parser.error("invalid generated frame grid")
    entries = [entry for entry in manifest["cases"] if entry["case"] == args.case]
    if len(entries) != 1:
        parser.error("expected one manifest entry for the selected case")
    entry = entries[0]
    for field in ("clip", "reference"):
        if Path(entry[field]).name != entry[field]:
            parser.error("generated manifest requires local filenames")
        if scorer["digest"](args.manifest.parent / entry[field]) != entry[f"{field}_sha256"]:
            parser.error(f"generated {field} hash differs")
    actions = json.loads((args.manifest.parent / entry["reference"]).read_text())["actions"]
    expected_times = np.arange(frames) / fps
    if len(actions) != frames or any(
        action["at"] != round(index * 1000 / fps)
        or not isinstance(action["pos"], int) or not 0 <= action["pos"] <= 100
        for index, action in enumerate(actions)
    ):
        parser.error("generated labels differ from the declared frame grid")
    model_hash = scorer["digest"](args.model)
    times, estimates, _ = scorer["read_recording"](args.recording, model_hash, entry["clip_sha256"])
    covered = (times >= expected_times[0]) & (times <= expected_times[-1])
    if covered.sum() < 2:
        parser.error("fewer than two estimates overlap the reference")
    observed = estimates[covered]
    expected = np.interp(times[covered], expected_times, [action["pos"] for action in actions])
    report = {
        "scope": "browser recording on independent synthetic outline geometry",
        "case": args.case,
        "source_sha256": entry["clip_sha256"],
        "reference_sha256": entry["reference_sha256"],
        "model_sha256": model_hash,
        "recording_sha256": scorer["digest"](args.recording),
        "playback_offset_ms": 0,
        "accepted_fresh_estimates": len(estimates),
        "scored_estimates": len(observed),
        "mean_absolute_error": float(np.abs(observed - expected).mean()),
        "pearson": (float(np.corrcoef(observed, expected)[0, 1])
                    if np.ptp(observed) and np.ptp(expected) else None),
        "physical_device_test": False,
    }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
