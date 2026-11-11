#!/usr/bin/env python3
"""Compare visible outline movement with the supplied sterile script at zero shift."""

import argparse
import json
from pathlib import Path
import runpy
import subprocess

import numpy as np


WIDTH, HEIGHT, FRAMES = 480, 270, 171
RIGHT_BAND_PX = 24
THRESHOLDS = (70, 85, 100, 120)
CLIP_SHA256 = "71d381468f83fd6e4116bb5d98b1c4453431702d1444e90b3c16d9d6d318494f"


def decode(clip):
    raw = subprocess.check_output(
        ["ffmpeg", "-nostdin", "-v", "error", "-i", clip, "-map", "0:v:0",
         "-vf", f"scale={WIDTH}:{HEIGHT}:flags=area", "-fps_mode", "passthrough",
         "-frames:v", str(FRAMES + 1), "-f", "rawvideo", "-pix_fmt", "gray", "pipe:1"],
        timeout=15,
    )
    frames = np.frombuffer(raw, np.uint8).reshape(-1, HEIGHT, WIDTH)
    probed = json.loads(subprocess.check_output(
        ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
         "frame=best_effort_timestamp_time", "-of", "json", clip], timeout=10,
    ))
    times = np.array([float(frame["best_effort_timestamp_time"]) for frame in probed["frames"]])
    if len(frames) != FRAMES or len(times) != FRAMES or not np.allclose(times, np.arange(FRAMES) / 12):
        raise ValueError("expected the exact sterile source frame grid")
    return frames, times


def proxy(frames, threshold):
    points = []
    for frame in frames:
        y, x = np.nonzero(frame >= threshold)
        if not len(x):
            raise ValueError("outline mask is empty")
        band = x >= x.max() - RIGHT_BAND_PX
        points.append((100 * x[band].mean() / (WIDTH - 1),
                       100 * (1 - y[band].mean() / (HEIGHT - 1))))
    return np.array(points)


def summary(observed, expected):
    return {
        "mae_points": float(np.abs(observed - expected).mean()),
        "pearson": float(np.corrcoef(observed, expected)[0, 1]),
    }


def span(values):
    return [float(values.min()), float(values.max())]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recording", type=Path)
    parser.add_argument("model", type=Path)
    parser.add_argument("sterile_reference", type=Path)
    parser.add_argument("output", type=Path, help="New diagnostic output directory")
    args = parser.parse_args()
    scorer = runpy.run_path(str(Path(__file__).with_name("score-sterile-browser-recording.py")))
    clip = args.sterile_reference / "handonly_sync_animation.mp4"
    if scorer["digest"](clip) != CLIP_SHA256:
        raise ValueError("expected the original sterile source")
    model_hash = scorer["digest"](args.model)
    reference = scorer["read_reference"](args.sterile_reference)
    recorded_times, estimates, _ = scorer["read_recording"](args.recording, model_hash, CLIP_SHA256)
    frames, source_times = decode(clip)
    reference_times = np.array([point["at"] / 1000 for point in reference])
    reference_positions = np.array([point["pos"] for point in reference])
    covered = ((recorded_times >= reference_times[0]) & (recorded_times <= reference_times[-1])
               & (recorded_times >= source_times[0]) & (recorded_times <= source_times[-1]))
    times, estimates = recorded_times[covered], estimates[covered]
    if len(times) < 2:
        raise ValueError("fewer than two estimates overlap the frame and reference grid")
    expected = np.interp(times, reference_times, reference_positions)
    sweep = {}
    for threshold in THRESHOLDS:
        xy = proxy(frames, threshold)
        x = np.interp(times, source_times, xy[:, 0])
        y = np.interp(times, source_times, xy[:, 1])
        sweep[str(threshold)] = {
            "visual_y_span": span(y),
            "visual_y_vs_supplied_script": summary(y, expected),
            "visual_x_vs_supplied_script": summary(x, expected),
            "tracker_vs_visual_y": summary(estimates, y),
        }

    report = {
        "scope": "diagnostic visual proxy, not an annotated motion reference",
        "source_sha256": CLIP_SHA256,
        "model_sha256": model_hash,
        "recording_sha256": scorer["digest"](args.recording),
        "reference_sha256": scorer["digest"](args.sterile_reference / "353079142771.funscript"),
        "playback_offset_ms": 0,
        "fitted_alignment": False,
        "scored_estimates": len(times),
        "supplied_script_span": span(expected),
        "tracker_vs_supplied_script": summary(estimates, expected),
        "proxy": "mean coordinate of bright pixels within 24 px of the rightmost bright pixel",
        "threshold_sweep": sweep,
        "physical_device_test": False,
    }
    args.output.mkdir(mode=0o700)
    (args.output / "result.json").write_text(json.dumps(report, indent=2) + "\n")

    import matplotlib
    matplotlib.use("Agg")
    from matplotlib import pyplot as plt

    figure, axes = plt.subplots(figsize=(10, 4))
    axes.plot(times, expected, label="Supplied script", linewidth=1.5)
    axes.plot(times, estimates, label="Browser tracker", linewidth=1.4)
    axes.plot(times, np.interp(times, source_times, proxy(frames, 85)[:, 1]),
              label="Visual Y proxy", linestyle="--", linewidth=1.4)
    axes.set(xlabel="Source time (s)", ylabel="Position (0–100)", ylim=(0, 100))
    axes.legend(loc="lower right")
    figure.tight_layout()
    figure.savefig(args.output / "comparison.png", dpi=150)
    plt.close(figure)


if __name__ == "__main__":
    main()
