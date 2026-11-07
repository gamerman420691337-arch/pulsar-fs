"""Check the independent whole-image translation fixture; never qualify a model."""
import argparse
import json
import math
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    assert report["qualified"] is False
    assert len(report["trials"]) == 2
    failures, unavailable, pairs = [], [], []
    for trial in report["trials"]:
        label = trial["label"]
        expected = report.get("shift_per_frame", 6) * trial["analysis_dimensions"][1] / 1080
        tracks = trial["selected_tracks"]
        if any(track is None for track in tracks) or len(set(tracks)) != 1:
            unavailable.append(label + ": continuous target identity unavailable")
        assert len(trial["separation"]) == 4
        assert trial["separation"][0] is None
        assert trial["evidence"][0] == "synthesized"
        for frame, separation in enumerate(trial["separation"][1:], 1):
            if separation is None or any(separation[k] is None for k in (
                    "target_image_translation", "background_image_translation", "target_relative_to_background")):
                unavailable.append(f"{label}: pair {frame} lacks consistent support")
                continue
            target = separation["target_image_translation"]
            background = separation["background_image_translation"]
            relative = separation["target_relative_to_background"]
            assert all(len(v) == 2 and all(math.isfinite(x) for x in v) for v in (target, background, relative))
            assert 3 <= separation["target_accepted_points"] <= 64
            assert 3 <= separation["background_accepted_points"] <= 64
            error = max(abs(target[0]), abs(background[0]), abs(target[1] - expected),
                        abs(background[1] - expected), abs(relative[0]), abs(relative[1]))
            pairs.append({"label": label, "frame": frame, "expected_vertical_pixels": expected,
                          "maximum_pixel_error": error, "support": separation})
            if error > 0.5 or trial["evidence"][frame] != "inferred":
                failures.append(f"{label}: pair {frame} does not match whole-image translation")
        if trial["actions"] is None:
            unavailable.append(label + ": standard export unavailable")
        elif not trial["actions"] or any(action["pos"] != 50 for action in trial["actions"]):
            failures.append(label + ": pure camera motion invented stroke travel")
    result = "FAIL" if failures else "PARTIAL" if unavailable else "PASS"
    args.output.write_text(json.dumps({"result": result, "qualified": False,
        "scope": "six native pairs from known public-image whole-frame shifts; not physical camera or target-quality qualification",
        "tolerance_pixels": 0.5, "supported_pairs": pairs, "failures": failures,
        "unavailable": unavailable}, indent=2) + "\n")
    print(json.dumps({"result": result, "supported_pairs": len(pairs), "qualified": False}))
    raise SystemExit(1 if failures else 2 if unavailable else 0)


if __name__ == "__main__":
    main()
