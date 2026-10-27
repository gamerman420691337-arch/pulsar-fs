"""Independent transformed-fixture oracle; not target or device qualification."""
import argparse
import json
import math
from pathlib import Path


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--report", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    a = p.parse_args()
    r = json.loads(a.report.read_text())
    assert r["qualified"] is False
    assert r["authored_patch"] == [24, 231, 800, 737]
    assert r["object_step_per_frame"] in (-6, 6)
    assert r["shift_per_frame"] == 6
    assert len(r["trials"]) == 2
    failures, unavailable, pairs = [], [], []
    for trial in r["trials"]:
        label = trial["label"]
        tracks = trial["selected_tracks"]
        if any(x is None for x in tracks) or len(set(tracks)) != 1:
            unavailable.append(label + ": continuous target identity unavailable")
        assert len(trial["separation"]) == 4
        assert trial["separation"][0] is None
        assert trial["evidence"][0] == "synthesized"
        scale = trial["analysis_dimensions"][1] / 1080
        expected_bg = 6 * scale
        expected_relative = r["object_step_per_frame"] * scale
        expected_target = expected_bg + expected_relative
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
            error = max(abs(target[0]), abs(background[0]), abs(relative[0]),
                        abs(target[1] - expected_target), abs(background[1] - expected_bg),
                        abs(relative[1] - expected_relative))
            pairs.append({"label": label, "frame": frame, "expected_target_pixels": expected_target,
                          "expected_background_pixels": expected_bg, "expected_relative_pixels": expected_relative,
                          "maximum_pixel_error": error, "support": separation})
            if error > 0.5 or trial["evidence"][frame] != "inferred":
                failures.append(f"{label}: pair {frame} does not preserve declared relative motion")
        actions = trial["actions"]
        expected_actions = [{"at": 500 * i, "pos": round(50 + i * r["object_step_per_frame"] * 100 / 1080)}
                            for i in range(4)]
        if actions is None:
            unavailable.append(label + ": standard export unavailable")
        elif actions != expected_actions:
            failures.append(label + ": standard actions differ from known relative motion")
    result = "FAIL" if failures else "PARTIAL" if unavailable else "PASS"
    a.output.write_text(json.dumps({"result": result, "qualified": False,
        "scope": "authored patch and image translations, actual native chain; no target-model or physical qualification",
        "tolerance_pixels": 0.5, "supported_pairs": pairs, "failures": failures,
        "unavailable": unavailable}, indent=2) + "\n")
    print(json.dumps({"result": result, "supported_pairs": len(pairs), "qualified": False}))
    raise SystemExit(1 if failures else 2 if unavailable else 0)


if __name__ == "__main__":
    main()
