#!/usr/bin/env python3
"""
FunscriptFlow - Automated Funscript Generation via Optical Flow
Clean, modernized, and cross-platform implementation.
"""

from __future__ import annotations

import argparse
import concurrent.futures
from dataclasses import asdict, dataclass
import gc
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import threading
from typing import Any, Callable, Dict, List, Optional, Tuple

import cv2
from decord import VideoReader, cpu
import numpy as np


# ==============================================================================
# Configuration Dataclass
# ==============================================================================
@dataclass
class FlowConfig:
    threads: int = 8
    target_fps: float = 30.0
    detrend_window: float = 2.0
    norm_window: float = 3.0
    batch_size: int = 3000
    overwrite: bool = False
    vr_mode: bool = False
    pov_mode: bool = False
    balance_global: bool = True
    keyframe_reduction: bool = True
    cut_diff_threshold: float = 30.0
    cut_flow_threshold: float = 7.0

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "FlowConfig":
        valid_keys = {f.name for f in cls.__dataclass_fields__.values()}
        filtered = {k: v for k, v in d.items() if k in valid_keys}
        # Cast numerical types if strings
        if "threads" in filtered:
            filtered["threads"] = int(filtered["threads"])
        if "target_fps" in filtered:
            filtered["target_fps"] = float(filtered["target_fps"])
        if "detrend_window" in filtered:
            filtered["detrend_window"] = float(filtered["detrend_window"])
        if "norm_window" in filtered:
            filtered["norm_window"] = float(filtered["norm_window"])
        if "batch_size" in filtered:
            filtered["batch_size"] = int(filtered["batch_size"])
        return cls(**filtered)


# ==============================================================================
# Localization Strings
# ==============================================================================
DEFAULT_STRINGS: Dict[str, str] = {
    "app_title": "FunscriptFlow",
    "select_videos": "Select Videos",
    "select_folder": "Select Folder",
    "no_files_selected": "No files selected",
    "vr_mode": "VR Mode",
    "vr_mode_tooltip": "Check this to process the upper/left quadrant for VR 180/360 videos.",
    "overall_progress": "Overall Progress:",
    "current_video_progress": "Current Video Progress:",
    "advanced_settings": "Advanced Settings",
    "threads": "Threads:",
    "detrend_window": "Detrend window (sec):",
    "norm_window": "Norm window (sec):",
    "batch_size": "Batch size (frames):",
    "show_preview": "Show Preview",
    "show_advanced": "Show Advanced Settings",
    "overwrite_files": "Overwrite existing files",
    "run": "Run",
    "cancel": "Cancel",
    "readme": "Readme",
    "config_saved": "Config saved to {config_path}",
    "config_load_error": "Error loading config: {error}",
    "no_files_warning": "Please select one or more video files or a folder.",
    "cancelled_by_user": "Processing cancelled by user.",
    "batch_processing_complete": "Batch processing complete.",
    "funscript_saved": "Funscript saved: {output_path}",
    "skipping_file_exists": "Skipping {video_path}: {output_path} exists.",
    "log_error": "ERROR: Could not write output: {error}",
    "found_files": "Found {n} file(s).",
    "processing_file": "--- Processing file {current}/{total}: {video_path} ---",
    "pov_mode_tooltip": "Uses fixed bottom-center anchor point. Ideal for POV scenes with static camera.",
    "processing_completed_with_errors": "Processing completed with errors. See run.log for details.",
}


def load_strings(filename: str = "strings.json") -> Dict[str, str]:
    if os.path.exists(filename):
        try:
            with open(filename, "r", encoding="utf-8") as f:
                loaded = json.load(f)
                res = DEFAULT_STRINGS.copy()
                res.update(loaded)
                return res
        except Exception:
            pass
    return DEFAULT_STRINGS.copy()


STRINGS = load_strings()


# ==============================================================================
# Cross-Platform Utilities
# ==============================================================================
def open_file_externally(filepath: str) -> None:
    """Open a file with the system default application across OSes."""
    try:
        if sys.platform == "win32":
            os.startfile(filepath)  # type: ignore[attr-defined]
        elif sys.platform == "darwin":
            subprocess.run(["open", filepath], check=False)
        else:
            subprocess.run(["xdg-open", filepath], check=False)
    except Exception as e:
        print(f"Error opening {filepath}: {e}", file=sys.stderr)


# ==============================================================================
# Core Computer Vision & Motion Tracking Algorithms
# ==============================================================================
def detect_cut_frame_diff(
    prev_frame: np.ndarray, curr_frame: np.ndarray, threshold: float = 30.0
) -> bool:
    """
    Detect scene cut via L1 mean pixel difference between grayscale frames.
    """
    diff = float(np.mean(np.abs(curr_frame.astype(np.float32) - prev_frame.astype(np.float32))))
    return diff > threshold


def compute_farneback_flow(prev_gray: np.ndarray, curr_gray: np.ndarray) -> np.ndarray:
    """
    Compute dense optical flow using Farneback algorithm.
    Returns: float32 array of shape (H, W, 2).
    """
    return cv2.calcOpticalFlowFarneback(
        prev_gray,
        curr_gray,
        None,
        pyr_scale=0.5,
        levels=3,
        winsize=15,
        iterations=3,
        poly_n=5,
        poly_sigma=1.2,
        flags=0,
    )


def max_divergence(flow: np.ndarray) -> Tuple[float, float, float]:
    """
    Compute optical flow divergence div = du/dx + dv/dy.
    Returns the (x, y) coordinates of maximum absolute divergence and its value.
    """
    div = np.gradient(flow[..., 0], axis=1) + np.gradient(flow[..., 1], axis=0)
    y, x = np.unravel_index(np.argmax(np.abs(div)), div.shape)
    return float(x), float(y), float(div[y, x])


def precompute_flow_pair(
    p0: np.ndarray,
    p1: np.ndarray,
    pov_mode: bool = False,
    cut_diff_threshold: float = 30.0,
    cut_flow_threshold: float = 7.0,
) -> Dict[str, Any]:
    """
    Process a single frame pair:
    - Frame difference cut detection
    - Optical flow calculation
    - Divergence center estimation
    - Flow magnitude cut verification
    """
    # 1. Photometric cut detection
    is_cut_diff = detect_cut_frame_diff(p0, p1, cut_diff_threshold)

    # 2. Optical flow
    flow = compute_farneback_flow(p0, p1)

    # 3. Flow magnitude cut detection
    mag, _ = cv2.cartToPolar(flow[..., 0], flow[..., 1])
    mean_mag = float(np.mean(mag))
    is_cut = is_cut_diff or (mean_mag > cut_flow_threshold)

    # 4. Center estimation
    if pov_mode:
        center = (float(p0.shape[1] // 2), float(p0.shape[0] - 1))
        div_val = 0.0
    else:
        cx, cy, div_val = max_divergence(flow)
        center = (cx, cy)

    return {
        "flow": flow,
        "center": center,
        "div_val": div_val,
        "cut": is_cut,
        "mean_mag": mean_mag,
    }


def radial_motion_weighted(
    flow: np.ndarray,
    center: Tuple[float, float],
    is_cut: bool,
    pov_mode: bool = False,
    balance_global: bool = True,
) -> float:
    """
    Project 2D flow vectors onto radial direction from the detected motion center.
    Positive indicates outward motion (expansion), negative indicates inward motion (contraction).
    """
    if is_cut:
        return 0.0

    h, w, _ = flow.shape
    y_coords, x_coords = np.indices((h, w), dtype=np.float32)
    dx = x_coords - center[0]
    dy = y_coords - center[1]

    dot = flow[..., 0] * dx + flow[..., 1] * dy

    if pov_mode or not balance_global:
        return float(np.mean(dot))

    # Weighting scheme to dampen camera panning / global movement
    w_safe = max(w, 1)
    h_safe = max(h, 1)
    weighted_dot = np.where(x_coords > center[0], dot * (w_safe - x_coords) / w_safe, dot * x_coords / w_safe)
    weighted_dot = np.where(y_coords > center[1], weighted_dot * (h_safe - y_coords) / h_safe, weighted_dot * y_coords / h_safe)

    return float(np.mean(weighted_dot))


def filter_center_outliers(
    raw_centers: List[Tuple[float, float]], window_radius: int = 6
) -> List[Tuple[float, float]]:
    """
    Temporal median filtering to prevent wild center jumps from disrupting radial motion.
    Uses median over a [j - window_radius, j + window_radius] window.
    """
    filtered = []
    n = len(raw_centers)
    arr = np.array(raw_centers, dtype=np.float32)

    for j in range(n):
        start = max(0, j - window_radius)
        end = min(n, j + window_radius + 1)
        sub = arr[start:end]
        med = np.median(sub, axis=0)
        filtered.append((float(med[0]), float(med[1])))

    return filtered


# ==============================================================================
# Signal Integration, Detrending & Normalization
# ==============================================================================
def integrate_flow(
    flow_entries: List[Tuple[float, bool, int]]
) -> Tuple[np.ndarray, np.ndarray]:
    """
    Trapezoidal/midpoint integration over flow velocities. Resets on cuts.
    Input entries: (dot_val, is_cut, frame_index)
    Returns: (integrated_displacement, frame_indices)
    """
    if not flow_entries:
        return np.array([]), np.array([])

    cum_flow = [0.0]
    indices = [flow_entries[0][2]]

    for i in range(1, len(flow_entries)):
        flow_prev, _, _ = flow_entries[i - 1]
        flow_curr, cut_curr, t_curr = flow_entries[i]

        if cut_curr:
            cum_flow.append(0.0)
        else:
            mid_flow = (flow_prev + flow_curr) / 2.0
            cum_flow.append(cum_flow[-1] + mid_flow)
        indices.append(t_curr)

    cum_arr = np.array(cum_flow, dtype=np.float32)
    # Correct residual phase offset with half-step blending, respecting cut boundaries
    for i in range(1, len(cum_arr)):
        if not flow_entries[i][1]:  # not cut
            cum_arr[i] = (cum_flow[i] + cum_flow[i - 1]) / 2.0
        else:
            cum_arr[i] = 0.0

    return cum_arr, np.array(indices, dtype=np.int64)


def detrend_and_normalize(
    cum_flow: np.ndarray,
    effective_fps: float,
    detrend_window_sec: float = 2.0,
    norm_window_sec: float = 3.0,
    discontinuity_threshold: float = 1000.0,
) -> np.ndarray:
    """
    Remove baseline drift using overlapping Hanning-windowed linear detrending,
    smooth high-frequency noise, and apply rolling min-max normalization into [0, 100].
    """
    n = len(cum_flow)
    if n == 0:
        return np.array([])

    detrend_win = max(5, int(detrend_window_sec * effective_fps))
    detrended = np.zeros(n, dtype=np.float32)
    weight_sum = np.zeros(n, dtype=np.float32)

    # Detect major jump boundaries
    diffs = np.abs(np.diff(cum_flow))
    disc_indices = np.where(diffs > discontinuity_threshold)[0] + 1
    boundaries = [0] + list(disc_indices) + [n]

    overlap = max(1, detrend_win // 2)

    for i in range(len(boundaries) - 1):
        s_start = boundaries[i]
        s_end = boundaries[i + 1]
        seg_len = s_end - s_start

        if seg_len < 5:
            detrended[s_start:s_end] = cum_flow[s_start:s_end] - np.mean(cum_flow[s_start:s_end])
            weight_sum[s_start:s_end] = 1.0
            continue

        if seg_len <= detrend_win:
            x = np.arange(seg_len, dtype=np.float32)
            seg = cum_flow[s_start:s_end]
            trend = np.polyfit(x, seg, 1)
            detrended_seg = seg - np.polyval(trend, x)
            w = np.hanning(seg_len).astype(np.float32)
            detrended[s_start:s_end] += detrended_seg * w
            weight_sum[s_start:s_end] += w
        else:
            for start in range(s_start, s_end - overlap, overlap):
                end = min(start + detrend_win, s_end)
                cur_len = end - start
                if cur_len < 2:
                    continue
                x = np.arange(cur_len, dtype=np.float32)
                seg = cum_flow[start:end]
                trend = np.polyfit(x, seg, 1)
                detrended_seg = seg - np.polyval(trend, x)
                w = np.hanning(cur_len).astype(np.float32)
                detrended[start:end] += detrended_seg * w
                weight_sum[start:end] += w

    detrended /= np.maximum(weight_sum, 1e-6)

    # 5-tap Gaussian smoothing
    kernel = np.array([1 / 16, 1 / 4, 3 / 8, 1 / 4, 1 / 16], dtype=np.float32)
    smoothed = np.convolve(detrended, kernel, mode="same")

    # Rolling window min-max normalization
    norm_win = int(norm_window_sec * effective_fps)
    if norm_win % 2 == 0:
        norm_win += 1
    half_norm = max(1, norm_win // 2)

    normalized = np.empty_like(smoothed)
    for idx in range(n):
        w_start = max(0, idx - half_norm)
        w_end = min(n, idx + half_norm + 1)
        window = smoothed[w_start:w_end]
        w_min = float(window.min())
        w_max = float(window.max())
        span = w_max - w_min
        if span <= 1e-5:
            normalized[idx] = 50.0
        else:
            normalized[idx] = (smoothed[idx] - w_min) / span * 100.0

    return np.clip(normalized, 0.0, 100.0)


def extract_funscript_actions(
    norm_values: np.ndarray,
    frame_indices: np.ndarray,
    fps: float,
    keyframe_reduction: bool = True,
) -> List[Dict[str, int]]:
    """
    Extract keyframe action points.
    If keyframe_reduction is True, extracts inflection points (local peaks and valleys).
    Returns list of dicts: {"at": timestamp_ms, "pos": 0-100}
    """
    n = len(norm_values)
    if n == 0:
        return []

    if keyframe_reduction and n > 2:
        key_indices = [0]
        for i in range(1, n - 1):
            d1 = norm_values[i] - norm_values[i - 1]
            d2 = norm_values[i + 1] - norm_values[i]
            if (d1 < 0) != (d2 < 0):
                key_indices.append(i)
        key_indices.append(n - 1)
    else:
        key_indices = list(range(n))

    actions: List[Dict[str, int]] = []
    fps_safe = max(fps, 0.001)

    for ki in key_indices:
        t_ms = int(round((float(frame_indices[ki]) / fps_safe) * 1000.0))
        # Funscript standard: 100 is top/contracted, 0 is bottom/extended
        pos = int(np.clip(round(100.0 - norm_values[ki]), 0, 100))
        # Prevent redundant identical timestamps
        if actions and actions[-1]["at"] == t_ms:
            actions[-1]["pos"] = pos
        else:
            actions.append({"at": t_ms, "pos": pos})

    return actions


# ==============================================================================
# Frame Ingestion Worker
# ==============================================================================
def fetch_frames_chunk(
    video_path: str, chunk_indices: List[int], config: FlowConfig
) -> List[np.ndarray]:
    """Read a batch of frames from video using decord in grayscale."""
    frames_gray: List[np.ndarray] = []
    w = 512 if config.vr_mode else 256
    h = 512 if config.vr_mode else 256

    try:
        vr = VideoReader(video_path, ctx=cpu(0), num_threads=config.threads, width=w, height=h)
        batch = vr.get_batch(chunk_indices).asnumpy()
    except Exception as e:
        print(f"Error reading frames from {video_path}: {e}", file=sys.stderr)
        return []

    del vr
    gc.collect()

    for f in batch:
        if config.vr_mode:
            fh, fw, _ = f.shape
            # Take the bottom-left quadrant for typical VR 180 stereo formats
            crop = f[fh // 2 :, : fw // 2]
            gray = cv2.cvtColor(crop, cv2.COLOR_RGB2GRAY)
        else:
            gray = cv2.cvtColor(f, cv2.COLOR_RGB2GRAY)
        frames_gray.append(gray)

    return frames_gray


def _pair_worker(args: Tuple[np.ndarray, np.ndarray, bool, float, float]) -> Dict[str, Any]:
    """Helper worker for multiprocessing pool."""
    p0, p1, pov, cut_diff, cut_flow = args
    return precompute_flow_pair(
        p0, p1, pov_mode=pov, cut_diff_threshold=cut_diff, cut_flow_threshold=cut_flow
    )


def _dot_worker(args: Tuple[np.ndarray, Tuple[float, float], bool, bool, bool]) -> float:
    """Helper worker for radial dot product."""
    flow, center, is_cut, pov, balance = args
    return radial_motion_weighted(flow, center, is_cut, pov_mode=pov, balance_global=balance)


# ==============================================================================
# Video Processing Pipeline
# ==============================================================================
def process_video(
    video_path: str,
    config: FlowConfig,
    log_func: Callable[[str], None] = print,
    progress_callback: Optional[Callable[[int], None]] = None,
    cancel_flag: Optional[Callable[[], bool]] = None,
) -> bool:
    """
    Run complete processing pipeline on a single video file.
    Returns True if an error occurred, False on success.
    """
    base, _ = os.path.splitext(video_path)
    output_path = base + ".funscript"

    if os.path.exists(output_path) and not config.overwrite:
        log_func(STRINGS["skipping_file_exists"].format(video_path=video_path, output_path=output_path))
        return False

    log_func(f"Opening: {video_path}")
    try:
        vr = VideoReader(video_path, ctx=cpu(0), num_threads=config.threads)
        total_frames = len(vr)
        fps = float(vr.get_avg_fps())
        del vr
        gc.collect()
    except Exception as e:
        log_func(f"ERROR: Cannot read video metadata: {e}")
        return True

    if total_frames < 2:
        log_func("ERROR: Video has fewer than 2 frames.")
        return True

    # Compute frame step based on target FPS (default 30 FPS)
    step = max(1, int(round(fps / max(1.0, config.target_fps))))
    effective_fps = fps / step
    indices = list(range(0, total_frames, step))
    log_func(
        f"Native FPS: {fps:.2f} | Downsampled to: ~{effective_fps:.2f} FPS (step {step}) | {len(indices)} frames to process"
    )

    bracket_size = max(100, config.batch_size)
    final_flow_list: List[Tuple[float, bool, int]] = []

    # Single reusable ProcessPoolExecutor across all batches
    num_workers = max(1, config.threads)
    with concurrent.futures.ProcessPoolExecutor(max_workers=num_workers) as pool:
        for chunk_start in range(0, len(indices), bracket_size):
            if cancel_flag and cancel_flag():
                log_func(STRINGS["cancelled_by_user"])
                return False

            chunk = indices[chunk_start : chunk_start + bracket_size]
            if len(chunk) < 2:
                continue

            frames_gray = fetch_frames_chunk(video_path, chunk, config)
            if len(frames_gray) < 2:
                log_func(f"Warning: Failed to fetch frames for chunk starting at {chunk_start}")
                continue

            pairs = list(zip(frames_gray[:-1], frames_gray[1:]))
            frame_indices = chunk[:-1]

            # 1. Parallel optical flow + cut detection
            worker_args = [
                (p[0], p[1], config.pov_mode, config.cut_diff_threshold, config.cut_flow_threshold)
                for p in pairs
            ]
            precomputed = list(pool.map(_pair_worker, worker_args))

            # 2. Filter center outliers using temporal median
            raw_centers = [info["center"] for info in precomputed]
            filtered_centers = filter_center_outliers(raw_centers, window_radius=6)

            # 3. Parallel weighted radial motion projection
            dot_args = [
                (
                    precomputed[j]["flow"],
                    filtered_centers[j],
                    precomputed[j]["cut"],
                    config.pov_mode,
                    config.balance_global,
                )
                for j in range(len(precomputed))
            ]
            dot_vals = list(pool.map(_dot_worker, dot_args))

            for j, dot_val in enumerate(dot_vals):
                final_flow_list.append((dot_val, precomputed[j]["cut"], frame_indices[j]))

            if progress_callback:
                prog = min(100, int(100 * (chunk_start + len(chunk)) / len(indices)))
                progress_callback(prog)

    if not final_flow_list:
        log_func("ERROR: No flow vectors computed.")
        return True

    # 4. Integrate flow velocities
    cum_flow, frame_idx_arr = integrate_flow(final_flow_list)

    # 5. Detrend and normalize
    normalized = detrend_and_normalize(
        cum_flow,
        effective_fps=effective_fps,
        detrend_window_sec=config.detrend_window,
        norm_window_sec=config.norm_window,
    )

    # 6. Extract funscript keyframes
    actions = extract_funscript_actions(
        normalized,
        frame_idx_arr,
        fps=fps,
        keyframe_reduction=config.keyframe_reduction,
    )

    # 7. Write funscript JSON file
    funscript_data = {"version": "1.0", "actions": actions}
    try:
        with open(output_path, "w", encoding="utf-8") as f:
            json.dump(funscript_data, f, indent=2)
        log_func(STRINGS["funscript_saved"].format(output_path=output_path))
        log_func(f"Generated {len(actions)} actions in {output_path}")
        return False
    except Exception as e:
        log_func(STRINGS["log_error"].format(error=str(e)))
        return True


# ==============================================================================
# Headless CLI Runner
# ==============================================================================
def run_headless(input_path: str, config: FlowConfig) -> None:
    """Process a video file or folder in headless mode."""
    target = Path(input_path)
    if not target.exists():
        print(f"Error: Input path '{input_path}' does not exist.", file=sys.stderr)
        return

    video_exts = {".mp4", ".m4v", ".avi", ".mov", ".mkv", ".webm"}
    if target.is_dir():
        files = [str(p) for p in target.rglob("*") if p.suffix.lower() in video_exts]
    else:
        files = [str(target)] if target.suffix.lower() in video_exts else []

    if not files:
        print("No supported video files found.", file=sys.stderr)
        return

    print(STRINGS["found_files"].format(n=len(files)))
    for idx, f in enumerate(files):
        print(STRINGS["processing_file"].format(current=idx + 1, total=len(files), video_path=f))
        err = process_video(f, config, log_func=print)
        if err:
            print(f"Error processing {f}", file=sys.stderr)

    print(STRINGS["batch_processing_complete"])


# ==============================================================================
# Desktop GUI (Tkinter)
# ==============================================================================
def run_gui(initial_config: Optional[FlowConfig] = None) -> None:
    """Launch the graphical interface."""
    import tkinter as tk
    from tkinter import filedialog, messagebox
    import tkinter.ttk as ttk

    class ToolTip:
        def __init__(self, widget: tk.Widget, text: str = ""):
            self.widget = widget
            self.text = text
            self.tipwindow: Optional[tk.Toplevel] = None
            widget.bind("<Enter>", self.enter)
            widget.bind("<Leave>", self.leave)

        def enter(self, event: Any = None) -> None:
            if self.tipwindow or not self.text:
                return
            x = self.widget.winfo_rootx() + 20
            y = self.widget.winfo_rooty() + 25
            self.tipwindow = tw = tk.Toplevel(self.widget)
            tw.wm_overrideredirect(True)
            tw.wm_geometry(f"+{x}+{y}")
            label = tk.Label(
                tw,
                text=self.text,
                justify=tk.LEFT,
                background="#ffffe0",
                relief=tk.SOLID,
                borderwidth=1,
                font=("tahoma", "8", "normal"),
            )
            label.pack(ipadx=2, ipady=1)

        def leave(self, event: Any = None) -> None:
            if self.tipwindow:
                self.tipwindow.destroy()
                self.tipwindow = None

    class App:
        def __init__(self, master: tk.Tk):
            self.master = master
            master.title(STRINGS["app_title"])
            self.files: List[str] = []
            self.cancel_event = threading.Event()
            self.config = initial_config or FlowConfig()

            # Top selection frame
            top_frame = tk.Frame(master)
            top_frame.pack(fill=tk.X, padx=8, pady=6)
            tk.Button(top_frame, text=STRINGS["select_videos"], command=self.select_files).pack(side=tk.LEFT, padx=3)
            tk.Button(top_frame, text=STRINGS["select_folder"], command=self.select_folder).pack(side=tk.LEFT, padx=3)
            self.lbl_files = tk.Label(top_frame, text=STRINGS["no_files_selected"])
            self.lbl_files.pack(side=tk.LEFT, padx=8)

            # Modes frame
            mode_frame = tk.Frame(master)
            mode_frame.pack(fill=tk.X, padx=8, pady=4)
            self.vr_mode = tk.BooleanVar(value=self.config.vr_mode)
            chk_vr = tk.Checkbutton(mode_frame, text=STRINGS["vr_mode"], variable=self.vr_mode)
            chk_vr.pack(side=tk.LEFT, padx=3)
            ToolTip(chk_vr, STRINGS["vr_mode_tooltip"])

            self.pov_mode = tk.BooleanVar(value=self.config.pov_mode)
            chk_pov = tk.Checkbutton(mode_frame, text="POV Mode", variable=self.pov_mode)
            chk_pov.pack(side=tk.LEFT, padx=3)
            ToolTip(chk_pov, STRINGS["pov_mode_tooltip"])

            self.balance_global = tk.BooleanVar(value=self.config.balance_global)
            chk_bal = tk.Checkbutton(mode_frame, text="Balance Global Motion", variable=self.balance_global)
            chk_bal.pack(side=tk.LEFT, padx=3)

            # Progress bars
            prog_frame = tk.Frame(master)
            prog_frame.pack(fill=tk.X, padx=8, pady=6)
            tk.Label(prog_frame, text=STRINGS["overall_progress"]).pack(anchor=tk.W)
            self.overall_progress = ttk.Progressbar(prog_frame, orient="horizontal", mode="determinate", maximum=100)
            self.overall_progress.pack(fill=tk.X, pady=2)

            tk.Label(prog_frame, text=STRINGS["current_video_progress"]).pack(anchor=tk.W)
            self.video_progress = ttk.Progressbar(prog_frame, orient="horizontal", mode="determinate", maximum=100)
            self.video_progress.pack(fill=tk.X, pady=2)

            # Advanced Settings Frame
            self.adv_frame = tk.LabelFrame(master, text=STRINGS["advanced_settings"])
            self.adv_frame.pack(fill=tk.X, padx=8, pady=6)

            self.params: Dict[str, tk.Entry] = {}
            cores = max(1, os.cpu_count() or 4)
            self.create_param(self.adv_frame, STRINGS["threads"], "threads", str(cores), "Worker threads for flow calculations")
            self.create_param(self.adv_frame, STRINGS["detrend_window"], "detrend_window", str(self.config.detrend_window), "Detrend window (seconds)")
            self.create_param(self.adv_frame, STRINGS["norm_window"], "norm_window", str(self.config.norm_window), "Normalization window (seconds)")
            self.create_param(self.adv_frame, STRINGS["batch_size"], "batch_size", str(self.config.batch_size), "Frame chunk size per batch")

            self.keyframe_reduction = tk.BooleanVar(value=self.config.keyframe_reduction)
            tk.Checkbutton(self.adv_frame, text="Enable keyframe reduction", variable=self.keyframe_reduction).pack(anchor=tk.W, padx=5, pady=2)

            self.overwrite = tk.BooleanVar(value=self.config.overwrite)
            tk.Checkbutton(self.adv_frame, text=STRINGS["overwrite_files"], variable=self.overwrite).pack(anchor=tk.W, padx=5, pady=2)

            # Action buttons
            btn_frame = tk.Frame(master)
            btn_frame.pack(padx=8, pady=8)
            self.btn_run = tk.Button(btn_frame, text=STRINGS["run"], command=self.run_batch, width=12, bg="#e1f5fe")
            self.btn_run.pack(side=tk.LEFT, padx=6)
            self.btn_cancel = tk.Button(btn_frame, text=STRINGS["cancel"], command=self.cancel_run, width=12, state="disabled")
            self.btn_cancel.pack(side=tk.LEFT, padx=6)

        def create_param(self, parent: tk.Widget, label_text: str, key: str, default: str, tip: str) -> None:
            frm = tk.Frame(parent)
            frm.pack(fill=tk.X, padx=5, pady=2)
            tk.Label(frm, text=label_text, width=22, anchor=tk.W).pack(side=tk.LEFT)
            entry = tk.Entry(frm)
            entry.insert(0, default)
            entry.pack(side=tk.LEFT, fill=tk.X, expand=True)
            self.params[key] = entry
            ToolTip(entry, tip)

        def select_files(self) -> None:
            selected = filedialog.askopenfilenames(
                title=STRINGS["select_videos"],
                filetypes=[("Video Files", "*.mp4 *.avi *.mov *.mkv *.m4v *.webm"), ("All Files", "*.*")],
            )
            if selected:
                self.files = list(selected)
                self.lbl_files.config(text=f"{len(self.files)} file(s) selected")

        def select_folder(self) -> None:
            folder = filedialog.askdirectory(title=STRINGS["select_folder"])
            if folder:
                video_exts = {".mp4", ".avi", ".mov", ".mkv", ".m4v", ".webm"}
                found = [
                    os.path.join(root, f)
                    for root, _, files in os.walk(folder)
                    for f in files
                    if os.path.splitext(f)[1].lower() in video_exts
                ]
                self.files = found
                self.lbl_files.config(text=f"{len(self.files)} file(s) found")

        def cancel_run(self) -> None:
            self.cancel_event.set()

        def set_ui_state(self, running: bool) -> None:
            self.btn_run.config(state="disabled" if running else "normal")
            self.btn_cancel.config(state="normal" if running else "disabled")

        def run_batch(self) -> None:
            if not self.files:
                messagebox.showwarning("No files", STRINGS["no_files_warning"])
                return

            try:
                cfg = FlowConfig(
                    threads=int(self.params["threads"].get()),
                    detrend_window=float(self.params["detrend_window"].get()),
                    norm_window=float(self.params["norm_window"].get()),
                    batch_size=int(self.params["batch_size"].get()),
                    overwrite=self.overwrite.get(),
                    vr_mode=self.vr_mode.get(),
                    pov_mode=self.pov_mode.get(),
                    balance_global=self.balance_global.get(),
                    keyframe_reduction=self.keyframe_reduction.get(),
                )
            except Exception as e:
                messagebox.showerror("Parameter Error", f"Invalid parameters: {e}")
                return

            self.cancel_event.clear()
            self.set_ui_state(running=True)
            self.overall_progress.config(value=0)
            self.video_progress.config(value=0)

            def worker() -> None:
                total = len(self.files)
                has_error = False

                for idx, video in enumerate(self.files):
                    if self.cancel_event.is_set():
                        break

                    self.master.after(0, lambda: self.video_progress.config(value=0))
                    err = process_video(
                        video,
                        cfg,
                        log_func=print,
                        progress_callback=lambda p: self.master.after(0, lambda: self.video_progress.config(value=p)),
                        cancel_flag=lambda: self.cancel_event.is_set(),
                    )
                    if err:
                        has_error = True

                    overall = int(100 * (idx + 1) / total)
                    self.master.after(0, lambda o=overall: self.overall_progress.config(value=o))

                def on_done() -> None:
                    self.set_ui_state(running=False)
                    if has_error:
                        messagebox.showwarning("Finished", STRINGS["processing_completed_with_errors"])
                    else:
                        messagebox.showinfo("Finished", STRINGS["batch_processing_complete"])

                self.master.after(0, on_done)

            threading.Thread(target=worker, daemon=True).start()

    root = tk.Tk()
    App(root)
    root.mainloop()


# ==============================================================================
# CLI Entry Point
# ==============================================================================
def main() -> None:
    parser = argparse.ArgumentParser(
        description="FunscriptFlow - Automated Funscript Generation via Optical Flow",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("input", nargs="?", help="Input video file or folder to process")
    parser.add_argument("--threads", type=int, default=min(8, os.cpu_count() or 4), help="Worker threads")
    parser.add_argument("--fps", "--target-fps", type=float, default=30.0, dest="target_fps", help="Target downsample FPS")
    parser.add_argument("--detrend-window", type=float, default=2.0, help="Detrend window in seconds")
    parser.add_argument("--norm-window", type=float, default=3.0, help="Normalization window in seconds")
    parser.add_argument("--batch-size", type=int, default=3000, help="Batch size in frames")
    parser.add_argument("--overwrite", action="store_true", default=False, help="Overwrite existing funscript files")
    parser.add_argument("--vr-mode", action="store_true", default=False, help="Enable VR mode (top-left / bottom-left crop)")
    parser.add_argument("--pov-mode", action="store_true", default=False, help="Enable POV mode (fixed bottom-center anchor)")
    parser.add_argument("--no-balance-global", action="store_false", dest="balance_global", default=True, help="Disable global motion compensation")
    parser.add_argument("--no-keyframe-reduction", action="store_false", dest="keyframe_reduction", default=True, help="Disable keyframe reduction")
    parser.add_argument("--gui", action="store_true", default=False, help="Force launch GUI mode")

    args = parser.parse_args()

    cfg = FlowConfig(
        threads=args.threads,
        target_fps=args.target_fps,
        detrend_window=args.detrend_window,
        norm_window=args.norm_window,
        batch_size=args.batch_size,
        overwrite=args.overwrite,
        vr_mode=args.vr_mode,
        pov_mode=args.pov_mode,
        balance_global=args.balance_global,
        keyframe_reduction=args.keyframe_reduction,
    )

    if args.input and not args.gui:
        run_headless(args.input, cfg)
    else:
        run_gui(cfg)


if __name__ == "__main__":
    main()
