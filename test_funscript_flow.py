#!/usr/bin/env python3
"""
Unit tests for FunscriptFlow modernized pipeline.
"""

import os
import sys
import tempfile
import numpy as np
import pytest

from funscript_flow import (
    FlowConfig,
    detect_cut_frame_diff,
    compute_farneback_flow,
    max_divergence,
    radial_motion_weighted,
    filter_center_outliers,
    integrate_flow,
    detrend_and_normalize,
    extract_funscript_actions,
)


def test_detect_cut_frame_diff():
    # Identical frames -> no cut
    f1 = np.full((100, 100), 128, dtype=np.uint8)
    f2 = np.full((100, 100), 128, dtype=np.uint8)
    assert not detect_cut_frame_diff(f1, f2, threshold=30.0)

    # Minor noise -> no cut
    f_noise = f1.copy()
    f_noise[::2, ::2] = 135
    assert not detect_cut_frame_diff(f1, f_noise, threshold=30.0)

    # Large sudden change (e.g. cut to black) -> cut detected
    f_black = np.zeros((100, 100), dtype=np.uint8)
    assert detect_cut_frame_diff(f1, f_black, threshold=30.0)


def test_compute_farneback_flow():
    p0 = np.zeros((64, 64), dtype=np.uint8)
    p0[20:40, 20:40] = 255

    # Shift square by 2 pixels to the right
    p1 = np.zeros((64, 64), dtype=np.uint8)
    p1[20:40, 22:42] = 255

    flow = compute_farneback_flow(p0, p1)
    assert flow.shape == (64, 64, 2)
    assert flow.dtype == np.float32

    # Vector in shifted region should have positive x velocity
    mid_flow_x = np.mean(flow[25:35, 25:35, 0])
    assert mid_flow_x > 0.1


def test_max_divergence():
    # Create an artificial radially expanding field centered at (32, 32)
    h, w = 64, 64
    y, x = np.indices((h, w), dtype=np.float32)
    center = (32.0, 32.0)
    dx = x - center[0]
    dy = y - center[1]

    # Flow pointing outward from center
    flow = np.zeros((h, w, 2), dtype=np.float32)
    flow[..., 0] = dx * 0.1
    flow[..., 1] = dy * 0.1

    cx, cy, div_val = max_divergence(flow)
    assert div_val > 0  # Net positive divergence (expansion)


def test_radial_motion_weighted():
    h, w = 64, 64
    y, x = np.indices((h, w), dtype=np.float32)
    center = (32.0, 32.0)
    dx = x - center[0]
    dy = y - center[1]

    # Expanding flow
    flow_out = np.zeros((h, w, 2), dtype=np.float32)
    flow_out[..., 0] = dx * 0.1
    flow_out[..., 1] = dy * 0.1
    dot_out = radial_motion_weighted(flow_out, center, is_cut=False, pov_mode=True)
    assert dot_out > 0.0

    # Contracting flow
    flow_in = -flow_out
    dot_in = radial_motion_weighted(flow_in, center, is_cut=False, pov_mode=True)
    assert dot_in < 0.0

    # Cut should produce 0.0
    assert radial_motion_weighted(flow_out, center, is_cut=True) == 0.0


def test_filter_center_outliers():
    raw = [(50.0, 50.0)] * 10
    # Inject a 1-frame outlier glitch at index 5
    raw[5] = (500.0, 500.0)

    filtered = filter_center_outliers(raw, window_radius=2)
    assert len(filtered) == 10
    # Median filter should have suppressed the single frame glitch completely
    assert abs(filtered[5][0] - 50.0) < 1.0
    assert abs(filtered[5][1] - 50.0) < 1.0


def test_integrate_flow():
    # 5 frames of positive motion followed by cut, followed by positive motion
    entries = [
        (10.0, False, 0),
        (10.0, False, 1),
        (10.0, False, 2),
        (0.0, True, 3),   # Cut!
        (10.0, False, 4),
        (10.0, False, 5),
    ]

    cum, indices = integrate_flow(entries)
    assert len(cum) == 6
    assert len(indices) == 6
    # Check that cut resets to 0.0
    assert cum[3] == 0.0
    # Subsequent values should integrate up from cut reset
    assert cum[5] > cum[3]


def test_detrend_and_normalize():
    # Create sinusoidal motion with linear upward drift
    t = np.linspace(0, 10, 300, dtype=np.float32)
    drift = 5.0 * t
    sine = 10.0 * np.sin(2 * np.pi * 1.0 * t)
    signal = drift + sine

    norm = detrend_and_normalize(signal, effective_fps=30.0, detrend_window_sec=2.0, norm_window_sec=2.0)
    assert len(norm) == len(signal)
    assert np.all(norm >= 0.0)
    assert np.all(norm <= 100.0)
    # The normalized signal should span most of the 0-100 range
    assert np.ptp(norm) > 50.0


def test_extract_funscript_actions():
    # Triangular wave: 0 -> 100 -> 0 -> 100
    norm_values = np.array([0, 25, 50, 75, 100, 75, 50, 25, 0, 25, 50, 75, 100], dtype=np.float32)
    frame_indices = np.arange(len(norm_values))
    fps = 30.0

    # With keyframe reduction, only extrema (inflections) should be extracted
    actions_reduced = extract_funscript_actions(norm_values, frame_indices, fps=fps, keyframe_reduction=True)
    assert len(actions_reduced) < len(norm_values)
    # Peaks in norm_values (100) become Funscript position 0 (100 - 100)
    # Valleys in norm_values (0) become Funscript position 100 (100 - 0)
    positions = [a["pos"] for a in actions_reduced]
    assert 0 in positions
    assert 100 in positions

    # Timestamps must be strictly ascending
    timestamps = [a["at"] for a in actions_reduced]
    assert timestamps == sorted(timestamps)
    assert len(timestamps) == len(set(timestamps))


def test_flow_config_defaults_and_casts():
    d = {
        "threads": "4",
        "target_fps": "29.97",
        "detrend_window": "2.5",
        "norm_window": "4.0",
        "batch_size": "2000",
        "overwrite": True,
        "vr_mode": False,
        "unknown_extra_arg": 123,
    }
    cfg = FlowConfig.from_dict(d)
    assert cfg.threads == 4
    assert abs(cfg.target_fps - 29.97) < 1e-3
    assert cfg.batch_size == 2000
    assert cfg.overwrite is True
    assert not hasattr(cfg, "unknown_extra_arg")


def test_end_to_end_synthetic_video():
    from funscript_flow import process_video
    import json
    import cv2

    with tempfile.TemporaryDirectory() as tmpdir:
        video_path = os.path.join(tmpdir, "test_synth.mp4")
        funscript_path = os.path.join(tmpdir, "test_synth.funscript")

        # Generate a 2-second 30fps test video with an expanding/contracting circle
        fps = 30.0
        width, height = 256, 256
        fourcc = cv2.VideoWriter_fourcc(*"mp4v")
        writer = cv2.VideoWriter(video_path, fourcc, fps, (width, height), isColor=True)

        for frame_num in range(60):
            frame = np.zeros((height, width, 3), dtype=np.uint8)
            # Oscillating circle radius between 10 and 50
            radius = int(30 + 20 * np.sin(2 * np.pi * frame_num / 15.0))
            if frame_num == 30:
                # Flash frame (scene cut simulation)
                frame.fill(255)
            else:
                cv2.circle(frame, (width // 2, height // 2), radius, (255, 255, 255), -1)
            writer.write(frame)
        writer.release()

        # Run process_video
        cfg = FlowConfig(
            threads=2,
            target_fps=30.0,
            detrend_window=1.0,
            norm_window=1.0,
            batch_size=100,
            overwrite=True,
            pov_mode=False,
            keyframe_reduction=True,
        )

        err = process_video(video_path, cfg, log_func=lambda msg: None)
        assert not err
        assert os.path.exists(funscript_path)

        with open(funscript_path, "r", encoding="utf-8") as f:
            data = json.load(f)

        assert data["version"] == "1.0"
        assert "actions" in data
        assert len(data["actions"]) > 0

        # Validate actions
        last_at = -1
        for action in data["actions"]:
            assert "at" in action
            assert "pos" in action
            assert 0 <= action["pos"] <= 100
            assert action["at"] > last_at
            last_at = action["at"]


if __name__ == "__main__":
    pytest.main(["-v", __file__])

