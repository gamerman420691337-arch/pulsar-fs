"""Actual CLI/Engine/worker letterbox exercise in an explicitly isolated fixture.

Use a private RAM TMPDIR and an offline namespace. No physical device effects.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import time


def main():
    p = argparse.ArgumentParser()
    for n in ("binary", "runtime-candidate", "pack", "image", "output"):
        p.add_argument("--" + n, type=Path, required=True)
    exercise = p.add_mutually_exclusive_group()
    exercise.add_argument("--exercise-score-thresholds", action="store_true")
    exercise.add_argument("--exercise-analysis-dimensions", action="store_true")
    exercise.add_argument("--exercise-camera-translation", action="store_true")
    exercise.add_argument("--exercise-relative-translation", action="store_true")
    p.add_argument("--object-step", type=int, choices=(-6, 6), default=6,
                   help="Signed original-pixel patch movement per frame for the relative-motion fixture")
    p.add_argument("--camera-step", type=int, choices=(6, 18, 36), default=6,
                   help="Original source-pixel shift per camera frame (default: 6)")
    a = p.parse_args()
    if a.camera_step != 6 and not (a.exercise_camera_translation or a.exercise_relative_translation):
        p.error("--camera-step requires a camera or relative translation exercise")
    if a.object_step != 6 and not a.exercise_relative_translation:
        p.error("--object-step requires --exercise-relative-translation")
    assert all(v.is_absolute() for v in vars(a).values() if isinstance(v, Path))
    assert a.output.is_dir()
    assert hashlib.sha256(a.image.read_bytes()).hexdigest() == "c02019c4979c191eb739ddd944445ef408dad5679acab6fd520ef9d434bfbc63"
    root = Path(tempfile.mkdtemp(prefix="pulsar-letterbox-", dir=os.environ["TMPDIR"]))
    env = {"PATH": "/usr/bin:/bin", "PULSAR_STATE_DIR": str(root / "state"),
           "PULSAR_ORT_DYLIB": str(a.runtime_candidate / "runtimes/libonnxruntime.so.1"),
           "PULSAR_FFMPEG": str(a.runtime_candidate / "runtimes/ffmpeg"),
           "PULSAR_FFPROBE": str(a.runtime_candidate / "runtimes/ffprobe")}
    records = []
    engine = None
    engine_log = None

    def cli(args, check=True):
        r = subprocess.run([str(a.binary), *args], env=env, capture_output=True, text=True, timeout=90)
        records.append({"args": args, "status": r.returncode, "stdout": r.stdout, "stderr": r.stderr})
        if check:
            assert r.returncode == 0, r.stderr
        return r

    try:
        engine_log = (root / "engine.log").open("w")
        engine = subprocess.Popen([str(a.binary), "engine"], env=env, stdout=subprocess.DEVNULL, stderr=engine_log)
        deadline = time.monotonic() + 30
        while not (root / "state/engine.sock").exists():
            assert engine.poll() is None and time.monotonic() < deadline
            time.sleep(0.05)
        original = json.loads((a.pack / "manifest.json").read_text())
        model = next(v for v in original["assets"] if v["role"] == "model")
        profile = json.loads(cli(["model", "profile", str(a.pack / model["path"]), "--width", "640",
                                  "--height", "640", "--class-count", "80", "--letterbox"]).stdout)
        assert profile["preprocessing"]["resize"] == "letterbox"
        assert profile["preprocessing"]["padding_byte"] == 114
        supplier = root / "supplier"
        shutil.copytree(a.pack, supplier)
        entry = next(v for v in original["assets"] if v["role"] == "profile")
        data = json.dumps(profile).encode()
        (supplier / entry["path"]).write_bytes(data)
        entry.update(byte_len=len(data), sha256=hashlib.sha256(data).hexdigest())
        original["pack_id"] = "ultralytics-yolo11n-letterbox-probe"
        original["source_description"] = "Pinned official generic COCO80 with explicit centered linear letterbox; unqualified"
        data = json.dumps(original).encode()
        (supplier / "manifest.json").write_bytes(data)
        identity = hashlib.sha256(data).hexdigest()
        imported = json.loads(cli(["model", "pack-import", str(supplier)]).stdout)
        assert imported["value"]["active"] is False
        selected = json.loads(cli(["model", "pack-select", identity, "--expected-selection-revision", "0",
                                   "--approve-unqualified-native-execution"]).stdout)
        assert selected["value"]["qualified"] is False
        video = root / "static-bus.y4m"
        conversion = subprocess.run([env["PULSAR_FFMPEG"], "-nostdin", "-hide_banner", "-loglevel", "error",
                                     "-loop", "1", "-i", str(a.image), "-frames:v", "4", "-r", "2",
                                     "-pix_fmt", "yuv420p", "-f", "yuv4mpegpipe", str(video)],
                                    capture_output=True, text=True, timeout=30)
        assert conversion.returncode == 0, conversion.stderr
        primary_dimensions = [] if a.exercise_analysis_dimensions else ["--width", "640", "--height", "640"]
        generation = cli(["generate", str(video), "--model-pack", identity, *primary_dimensions,
                          "--output", str(root / "output.funscript")], check=False)
        with sqlite3.connect(root / "state/projects.sqlite3") as db:
            jobs = db.execute("SELECT project,source,state FROM jobs").fetchall()
        assert len(jobs) == 1 and jobs[0][2] in ("completed", "review_required"), jobs
        dimension_trials = []
        if a.exercise_analysis_dimensions:
            def collect_dimensions(label, limits, decoded_dimensions, result):
                with sqlite3.connect(root / "state/projects.sqlite3") as db:
                    row = db.execute("SELECT attempt,state FROM jobs ORDER BY rowid DESC LIMIT 1").fetchone()
                assert row[1] in ("completed", "review_required"), row
                receipt_path = root / "state/attempts" / row[0] / "receipt.json"
                receipt = json.loads(receipt_path.read_text())
                configuration = receipt["configuration"]
                assert [configuration["analysis_width"], configuration["analysis_height"]] == limits
                assert receipt["analysis_dimensions"] == decoded_dimensions
                assert receipt["source_dimensions"] == [810, 1080]
                assert receipt["qualification"] == "unqualified"
                assert receipt["decoded_sample_count"] == 4
                assert [sample["source_frame_index"] for sample in receipt["samples"]] == [0, 1, 2, 3]
                assert [sample["project_time_ns"] for sample in receipt["samples"]] == [0, 500000000, 1000000000, 1500000000]
                shutil.copyfile(receipt_path, a.output / ("dimensions-" + label + "-lineage.json"))
                dimension_trials.append({"label": label, "limits": limits, "decoded_dimensions": decoded_dimensions,
                                         "generation_status": result.returncode, "job_state": row[1],
                                         "observed_boxes": [sample["observed_boxes"] for sample in receipt["samples"]],
                                         "project_time_ns": [sample["project_time_ns"] for sample in receipt["samples"]],
                                         "configuration_sha256": receipt["configuration_sha256"], "qualified": False})

            collect_dimensions("automatic", [640, 640], [480, 640], generation)
            for label, arguments, limits, decoded_dimensions in (
                    ("explicit-small", ["--width", "320", "--height", "180"], [320, 180], [135, 180]),
                    ("width-only", ["--width", "320"], [320, 640], [320, 426]),
                    ("height-only", ["--height", "180"], [640, 180], [135, 180])):
                trial = cli(["generate", str(video), "--model-pack", identity, *arguments,
                             "--output", str(root / (label + ".funscript"))], check=False)
                collect_dimensions(label, limits, decoded_dimensions, trial)
            for arguments in (["--width", "0"], ["--height", "16385"]):
                trial = cli(["generate", str(video), "--model-pack", identity, *arguments], check=False)
                assert trial.returncode != 0
                with sqlite3.connect(root / "state/projects.sqlite3") as db:
                    assert db.execute("SELECT COUNT(*) FROM jobs").fetchone()[0] == 4
        score_trials = []
        if a.exercise_score_thresholds:
            def collect_trial(expected, generation_result):
                with sqlite3.connect(root / "state/projects.sqlite3") as db:
                    rows = db.execute("SELECT id,attempt,manifest,state FROM jobs ORDER BY rowid DESC LIMIT 1").fetchall()
                assert len(rows) == 1 and rows[0][3] in ("completed", "review_required"), rows
                receipt_path = root / "state/attempts" / rows[0][1] / "receipt.json"
                receipt = json.loads(receipt_path.read_text())
                assert receipt["detector_score_threshold"] == expected
                assert receipt["qualification"] == "unqualified"
                if expected == 0.25:
                    assert "detector_score_threshold" not in receipt["configuration"]
                else:
                    assert receipt["configuration"]["detector_score_threshold"] == expected
                counts = [sample["observed_boxes"] for sample in receipt["samples"]]
                assert len(counts) == 4
                label = str(expected).replace(".", "-")
                shutil.copyfile(receipt_path, a.output / ("score-" + label + "-lineage.json"))
                score_trials.append({"threshold": expected, "observed_boxes": counts,
                                     "generation_status": generation_result.returncode,
                                     "configuration_sha256": receipt["configuration_sha256"],
                                     "job_state": rows[0][3], "qualified": False})

            collect_trial(0.25, generation)
            for score in (0.5, 1.0):
                trial = cli(["generate", str(video), "--model-pack", identity,
                             "--width", "640", "--height", "640", "--conf", str(score),
                             "--output", str(root / ("score-" + str(score) + ".funscript"))], check=False)
                collect_trial(score, trial)
            assert all(c > 0 for c in score_trials[0]["observed_boxes"])
            assert all(c <= low for c, low in zip(score_trials[1]["observed_boxes"], score_trials[0]["observed_boxes"]))
            assert score_trials[2]["observed_boxes"] == [0, 0, 0, 0]
            assert score_trials[2]["generation_status"] != 0, "unavailable target must refuse standard export"
            assert len({trial["configuration_sha256"] for trial in score_trials}) == 3
        camera_trials = []
        if a.exercise_camera_translation or a.exercise_relative_translation:
            original_rgb = subprocess.run([env["PULSAR_FFMPEG"], "-nostdin", "-hide_banner", "-loglevel", "error",
                                           "-i", str(a.image), "-frames:v", "1", "-pix_fmt", "rgb24",
                                           "-f", "rawvideo", "pipe:1"], capture_output=True, timeout=30)
            assert original_rgb.returncode == 0, original_rgb.stderr.decode()
            assert len(original_rgb.stdout) == 810 * 1080 * 3
            raw = root / "camera.rgb"
            with raw.open("wb") as output:
                for index in range(4):
                    source = bytearray(original_rgb.stdout)
                    if a.exercise_relative_translation:
                        # A deliberately authored rectangular patch, not an
                        # annotation or segmentation claim. Its interior moves
                        # independently; exposed rows are filled RGB114.
                        x0, x1, y0, y1 = 24, 800, 231, 737
                        offset = a.object_step * index
                        for y in range(y0, y1):
                            start = (y * 810 + x0) * 3
                            source[start:start + (x1 - x0) * 3] = bytes([114]) * ((x1 - x0) * 3)
                        for y in range(y0, y1):
                            src = (y * 810 + x0) * 3
                            dst = ((y + offset) * 810 + x0) * 3
                            source[dst:dst + (x1 - x0) * 3] = original_rgb.stdout[src:src + (x1 - x0) * 3]
                    shift = a.camera_step * index
                    output.write(bytes([114]) * (810 * shift * 3))
                    output.write(source[:810 * (1080 - shift) * 3])
            camera = root / "camera.y4m"
            encode = subprocess.run([env["PULSAR_FFMPEG"], "-nostdin", "-hide_banner", "-loglevel", "error",
                                     "-f", "rawvideo", "-pixel_format", "rgb24", "-video_size", "810x1080",
                                     "-framerate", "2", "-i", str(raw), "-frames:v", "4", "-pix_fmt", "yuv420p",
                                     "-f", "yuv4mpegpipe", str(camera)], capture_output=True, text=True, timeout=30)
            assert encode.returncode == 0, encode.stderr
            shutil.copyfile(camera, a.output / "controlled-camera.y4m")
            for label, arguments, dimensions in (("automatic", [], [480, 640]),
                                                  ("explicit-small", ["--width", "320", "--height", "180"], [135, 180])):
                export = root / ("camera-" + label + ".funscript")
                trial = cli(["generate", str(camera), "--model-pack", identity, *arguments,
                             "--output", str(export)], check=False)
                with sqlite3.connect(root / "state/projects.sqlite3") as db:
                    row = db.execute("SELECT attempt,state FROM jobs ORDER BY rowid DESC LIMIT 1").fetchone()
                assert row[1] in ("completed", "review_required"), row
                receipt_path = root / "state/attempts" / row[0] / "receipt.json"
                receipt = json.loads(receipt_path.read_text())
                assert receipt["qualification"] == "unqualified"
                assert receipt["analysis_dimensions"] == dimensions
                assert [sample["source_frame_index"] for sample in receipt["samples"]] == [0, 1, 2, 3]
                assert [sample["project_time_ns"] for sample in receipt["samples"]] == [0, 500000000, 1000000000, 1500000000]
                separation = [sample.get("motion_separation") for sample in receipt["samples"]]
                script = json.loads(export.read_text()) if trial.returncode == 0 else None
                if script is not None:
                    shutil.copyfile(export, a.output / ("camera-" + label + ".funscript"))
                shutil.copyfile(receipt_path, a.output / ("camera-" + label + "-lineage.json"))
                camera_trials.append({"label": label, "generation_status": trial.returncode, "job_state": row[1],
                                      "analysis_dimensions": dimensions, "separation": separation,
                                      "recipe": receipt["recipe"],
                                      "flow_search": [sample.get("flow_search") for sample in receipt["samples"]],
                                      "target_sampling_points": [sample.get("target_sampling_points") for sample in receipt["samples"]],
                                      "evidence": [sample["evidence"] for sample in receipt["samples"]],
                                      "selected_tracks": [sample["selected_track"] for sample in receipt["samples"]],
                                      "observed_boxes": [sample["observed_boxes"] for sample in receipt["samples"]],
                                      "reviews": receipt["review"], "unresolved_gaps": receipt["unresolved_gaps"],
                                      "actions": script["actions"] if script else None, "qualified": False})
            (a.output / "camera-trials.json").write_text(json.dumps({"qualified": False,
                "recipe": ("authored rectangular patch translated independently, then whole image translated; exposed pixels RGB114"
                           if a.exercise_relative_translation else
                           "public image shifted down by declared original-pixel step; top rows filled RGB114; no object-relative motion"),
                "object_step_per_frame": a.object_step if a.exercise_relative_translation else 0,
                "authored_patch": [24, 231, 800, 737] if a.exercise_relative_translation else None,
                "shift_per_frame": a.camera_step,
                "source_rgb_sha256": hashlib.sha256(original_rgb.stdout).hexdigest(),
                "encoded_sha256": hashlib.sha256(camera.read_bytes()).hexdigest(), "trials": camera_trials}, indent=2) + "\n")
        preview = json.loads(cli(["project", "preview-pack", jobs[0][0], "500000000", "--model-pack", identity,
                                  "--source-version", jobs[0][1]]).stdout)
        assert preview["value"]["qualified"] is False
        image_preview = preview["value"]["preview"]
        assert image_preview["analysis"]["status"] == "available"
        assert image_preview["observations"], "real preview must contain positive detections"
        assert all(o["context"] == image_preview["context"] for o in image_preview["observations"])
        for observation in image_preview["observations"]:
            bounds = observation["bounds"]
            assert bounds["space"] == "projection_normalized"
            assert 0 <= bounds["x_min"] < bounds["x_max"] <= 1
            assert 0 <= bounds["y_min"] < bounds["y_max"] <= 1
        frame = image_preview["frame"]
        assert frame["width"] * frame["height"] <= 640 * 640
        decoded = subprocess.run([env["PULSAR_FFMPEG"], "-nostdin", "-hide_banner", "-loglevel", "error",
                                  "-i", str(video), "-vf", f"scale={frame['width']}:{frame['height']}",
                                  "-pix_fmt", "rgb24", "-f", "rawvideo", "pipe:1"],
                                 capture_output=True, timeout=30)
        assert decoded.returncode == 0, decoded.stderr.decode()
        frame_bytes = frame["width"] * frame["height"] * 3
        assert len(decoded.stdout) == 4 * frame_bytes
        hashes = [hashlib.sha256(decoded.stdout[i:i + frame_bytes]).hexdigest()
                  for i in range(0, len(decoded.stdout), frame_bytes)]
        assert len(set(hashes)) == 1, "static source must decode to identical frames"
        assert hashlib.sha256(Path(frame["artifact"]["path"]).read_bytes()).hexdigest() == hashes[0]
        static_export = None
        if generation.returncode == 0:
            output = root / "output.funscript"
            script = json.loads(output.read_text())
            assert script["actions"] and len({v["pos"] for v in script["actions"]}) == 1, "static input invented stroke travel"
            shutil.copyfile(output, a.output / "static-output.funscript")
            static_export = {"sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                             "actions": len(script["actions"]), "position": script["actions"][0]["pos"],
                             "neutral_travel": 0, "qualified": False}
        report = {"qualified": False, "pack_sha256": identity, "profile": profile, "job": jobs[0], "score_trials": score_trials, "dimension_trials": dimension_trials, "camera_trials": camera_trials,
                  "generation_status": generation.returncode, "preview": preview,
                  "decoded_static_frame_sha256": hashes[0], "static_export": static_export,
                  "scope": "ordinary static COCO image, actual CLI/Engine/worker; no target motion quality or release claim"}
        (a.output / "process.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.copyfile(image_preview["frame"]["artifact"]["path"], a.output / "preview-frame.raw")
        print(json.dumps({"observations": len(image_preview["observations"]), "job_state": jobs[0][2], "qualified": False}))
    finally:
        if engine is not None and engine.poll() is None:
            engine.terminate()
            try:
                engine.wait(timeout=5)
            except subprocess.TimeoutExpired:
                engine.kill()
                engine.wait(timeout=5)
        if engine_log is not None:
            engine_log.close()
            shutil.copyfile(root / "engine.log", a.output / "engine.log")
        (a.output / "commands.json").write_text(json.dumps(records, indent=2) + "\n")
        shutil.rmtree(root)


if __name__ == "__main__":
    main()
