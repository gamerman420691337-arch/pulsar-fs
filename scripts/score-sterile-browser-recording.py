#!/usr/bin/env python3
"""Compare browser live positions against the supplied sterile reference at zero offset."""

import argparse
import hashlib
import itertools
import json
from pathlib import Path

import numpy as np

MAX_MAE_POINTS = 15
MIN_CORRELATION = 0.85
CLIP_DURATION_MS = 14_250


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_reference(folder):
    def actions(suffix):
        return [
            {"at": action["at"], "pos": action["pos"]}
            for action in json.loads((folder / f"353079142771.{suffix}").read_text())[
                "actions"
            ]
        ]

    script = actions("funscript")
    csv = [
        dict(zip(("at", "pos"), map(int, line.split(","))))
        for line in (folder / "353079142771.csv").read_text().splitlines()
        if line and not line.startswith("#")
    ]
    if script != csv or script != actions("json"):
        raise ValueError("Supplied references differ")
    if any(b["at"] <= a["at"] for a, b in itertools.pairwise(script)):
        raise ValueError("Reference timestamps must be strictly increasing")
    # Do not extend the final incomplete stroke past the video.
    return [action for action in script if action["at"] <= CLIP_DURATION_MS]


def read_recording(path, model_hash, source_hash):
    recording = json.loads(path.read_text())
    if recording.get("kind") != "pulsar-live-inference-recording" or recording.get(
        "qualified"
    ):
        raise ValueError("Expected an unqualified browser inference recording")
    times = []
    positions = []
    pages = recording["pages"]
    if not pages:
        raise ValueError("Recording has no pages")
    first = pages[0]
    identity_fields = (
        "identity",
        "engine_epoch",
        "recording_request_id",
        "inference_attempt",
        "browser_media",
        "profile",
        "total_samples",
    )
    for index, page in enumerate(pages):
        expected_next = len(times) + len(page["samples"])
        if (
            any(page[field] != first[field] for field in identity_fields)
            or not page["samples"]
            or page["discarded"]
            or page["exhausted"]
            or page["after"] != len(times)
            or page["next"] != (None if index == len(pages) - 1 else expected_next)
        ):
            raise ValueError("Recording pages are incomplete or belong to different captures")
        if (
            page["source_time_basis"] != "source_media_presentation"
            or page["active"]
            or page["profile"]["model"]["sha256"] != model_hash
            or page["browser_media"]["selection"]["source_version"] != source_hash
        ):
            raise ValueError(
                "Recording has a different clock, model, source, or running capture"
            )
        for row in page["samples"]:
            if not row["fresh"] or row["result"]["status"] != "estimated":
                raise ValueError("Recording contains stale or non-estimated samples")
            sample = row["result"]["payload"]
            time = sample["input"]["pair"]["current_time"]
            times.append(time["numerator"] / time["denominator"])
            positions.append(sample["position"] * 100)
    times, positions = np.array(times), np.array(positions)
    if (
        len(times) != first["total_samples"]
        or len(times) < 2
        or not np.all(np.isfinite(times))
        or np.any(np.diff(times) <= 0)
        or not np.all(np.isfinite(positions))
        or np.any((positions < 0) | (positions > 100))
    ):
        raise ValueError("Expected ordered samples with bounded finite positions")
    first_pair = first["samples"][0]["result"]["payload"]["input"]["pair"]
    anchor = first_pair["presentation_clock"]["source_anchor"]
    return times, positions, anchor["numerator"] / anchor["denominator"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recording", type=Path)
    parser.add_argument("model", type=Path)
    parser.add_argument("sterile_reference", type=Path)
    parser.add_argument(
        "output", type=Path, help="New directory for the score and comparison plot"
    )
    parser.add_argument("--product-commit", required=True)
    args = parser.parse_args()

    reference = read_reference(args.sterile_reference)
    clip = args.sterile_reference / "handonly_sync_animation.mp4"
    funscript = args.sterile_reference / "353079142771.funscript"
    model_hash, source_hash = digest(args.model), digest(clip)
    times, positions, source_anchor = read_recording(args.recording, model_hash, source_hash)
    last_reference_ms = reference[-1]["at"]
    scored = (times * 1000 >= reference[0]["at"]) & (times * 1000 <= last_reference_ms)
    if np.count_nonzero(scored) < 2:
        raise ValueError("Fewer than two samples overlap the reference")
    reference_times = np.array([point["at"] for point in reference])
    reference_positions = np.array([point["pos"] for point in reference])
    expected = np.interp(times[scored] * 1000, reference_times, reference_positions)
    observed = positions[scored]
    scored_times_ms = times[scored] * 1000
    mae = float(np.abs(observed - expected).mean())
    correlation = None
    if np.ptp(expected) and np.ptp(observed):
        correlation = float(np.corrcoef(observed, expected)[0, 1])
    passed = (
        mae <= MAX_MAE_POINTS
        and correlation is not None
        and correlation >= MIN_CORRELATION
    )
    result = {
        "status": "PASS_MOTION_SCREEN" if passed else "FAIL_MOTION_SCREEN",
        "physical_handy_test": False,
        "playback_offset_ms": 0,
        "rendered_browser": True,
        "engine_clock_basis": "source_media_presentation",
        "accepted_fresh_estimates": len(positions),
        "scored_estimates": len(expected),
        "first_sample_s": float(times[0]),
        "capture_source_anchor_s": source_anchor,
        "source_seconds_from_capture_anchor_to_first_estimate": float(times[0] - source_anchor),
        "last_sample_s": float(times[-1]),
        "reference_end_ms": last_reference_ms,
        "reference_start_unobserved_ms": float(scored_times_ms[0] - reference_times[0]),
        "reference_end_unobserved_ms": float(last_reference_ms - scored_times_ms[-1]),
        "largest_scored_sample_gap_ms": float(np.diff(scored_times_ms).max()),
        "normalized_position_mae_points": mae,
        "correlation": correlation,
        "sample_gap_median_ms": float(np.median(np.diff(times)) * 1000),
        "estimated_position_min": float(positions.min()),
        "estimated_position_max": float(positions.max()),
        "acceptance_screen": {
            "max_mae_points": MAX_MAE_POINTS,
            "min_pearson": MIN_CORRELATION,
        },
        "reference_alignment": "supplied timestamps, no offset or amplitude fit",
        "model_sha256": model_hash,
        "reference_actions_equal_json_csv_funscript": True,
        "inputs": {clip.name: source_hash, funscript.name: digest(funscript)},
        "product_source_commit": args.product_commit,
    }

    args.output.mkdir()
    (args.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots(figsize=(10, 3.6), layout="constrained")
    ax.plot(
        reference_times / 1000,
        reference_positions,
        color="#2a567e",
        lw=1.5,
        label="Supplied reference",
    )
    ax.plot(times, positions, color="#bc4b18", lw=1.7, label="Browser live estimate")
    ax.set(
        xlim=(0, CLIP_DURATION_MS / 1000),
        ylim=(-3, 103),
        xlabel="Source time (s)",
        ylabel="Position (0–100)",
        title="Sterile clip · Browser live estimates vs supplied reference",
    )
    ax.grid(axis="y", alpha=0.2)
    ax.legend(loc="lower left", frameon=False)
    fig.savefig(args.output / "motion-comparison.png", dpi=140)
    print(json.dumps(result, indent=2))
    return 0 if passed else 2


if __name__ == "__main__":
    raise SystemExit(main())
