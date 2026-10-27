#!/usr/bin/env python3
"""Exercise an installed candidate with authored sources; never certify an edition."""
import argparse
import hashlib
import json
import shutil
import subprocess
import time
from pathlib import Path


def identity(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(65536), b""):
            digest.update(block)
    return {"sha256": digest.hexdigest(), "byte_len": path.stat().st_size}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--audio-fixtures", type=Path, required=True)
    parser.add_argument("--video-fixtures", type=Path, required=True)
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    root = args.output.absolute()
    root.mkdir(mode=0o700)
    manifest = json.loads((runtime / "runtime-candidate.json").read_text())
    for entry in manifest["files"]:
        assert identity(runtime / entry["path"]) == {
            "sha256": entry["sha256"], "byte_len": entry["byte_len"]}
    binary, ffmpeg = runtime / "pulsar", runtime / "runtimes/ffmpeg"
    state = root / "state"
    environment = {"PATH": "/usr/bin:/bin", "PULSAR_STATE_DIR": str(state),
                   "TMPDIR": str(root), "HOME": str(root)}
    records, cases = [], []
    engine = None
    engine_log = (root / "engine.stderr").open("w")

    def invoke(arguments, success=True):
        result = subprocess.run([str(binary)] + list(map(str, arguments)),
                                env=environment, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=45)
        assert len(result.stdout) + len(result.stderr) <= 4 * 1024 * 1024
        record = {"arguments": list(map(str, arguments)), "exit": result.returncode,
                  "stdout": result.stdout, "stderr": result.stderr}
        records.append(record)
        (root / f"cli-{len(records):03}.json").write_text(json.dumps(record, indent=2) + "\n")
        if success:
            assert result.returncode == 0, record
        else:
            assert result.returncode != 0, record
        return result

    def ok(arguments):
        return json.loads(invoke(arguments).stdout)

    def tool(arguments):
        result = subprocess.run([str(ffmpeg)] + list(map(str, arguments)),
                                env=environment, stdin=subprocess.DEVNULL,
                                capture_output=True, timeout=30)
        assert result.returncode == 0, result.stderr

    def inspect_script(path):
        value = json.loads(path.read_text())
        actions = value["actions"]
        assert actions and all(isinstance(a["at"], int) and isinstance(a["pos"], int)
                               and a["at"] >= 0 and 0 <= a["pos"] <= 100 for a in actions)
        assert all(a["at"] < b["at"] for a, b in zip(actions, actions[1:]))
        return actions

    def generate(name, source, extra=()):
        output = root / f"{name}.funscript"
        invoke(["generate", source, "--profile", "fast",
                "--width", "64", "--height", "64", "--output", output, *extra])
        actions = inspect_script(output)
        cases.append({"name": name, "state": "PASS", "actions": len(actions),
                      "output": output.name, "output_identity": identity(output),
                      "scope": "authored Fast workflow; no learned target or quality qualification"})
        return actions

    try:
        for name in ["clean.m4a", "damaged-1.m4a"]:
            shutil.copyfile(args.audio_fixtures / name, root / name)
        for name in ["clean.mp4", "damaged-1.mp4"]:
            shutil.copyfile(args.video_fixtures / name, root / name)
        image = root / "still.ppm"
        image.write_bytes(b"P6\n64 64\n255\n" + bytes([64, 128, 192]) * 4096)
        mixed = root / "mixed.mp4"
        tool(["-v", "error", "-nostdin", "-i", root / "clean.mp4", "-i", root / "clean.m4a",
              "-map", "0:v:0", "-map", "1:a:0", "-c", "copy", mixed])
        for name, size in [("stereo", "128x64"), ("vr180", "256x128"), ("vr360", "256x64")]:
            tool(["-v", "error", "-nostdin", "-f", "lavfi", "-i",
                  f"testsrc2=size={size}:rate=8:duration=2", "-threads", "1",
                  "-c:v", "mpeg4", root / f"{name}.mp4"])
        engine = subprocess.Popen([str(binary), "engine"], env=environment,
                                  stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=engine_log)
        deadline = time.monotonic() + 15
        while not (state / "engine.sock").exists():
            assert engine.poll() is None, "owned Engine exited during startup"
            assert time.monotonic() < deadline, "owned Engine startup deadline"
            time.sleep(0.025)
        ok(["capabilities"])
        ordinary = generate("ordinary-video", root / "clean.mp4")
        viewport = {"yaw_degrees": 0, "pitch_degrees": 0, "roll_degrees": 0,
                    "horizontal_fov_degrees": 90, "vertical_fov_degrees": 90}
        for name, projection in [("stereo", {"kind": "flat"}),
                                 ("vr180", {"kind": "half_equirectangular180", "viewport": viewport}),
                                 ("vr360", {"kind": "equirectangular360", "viewport": viewport})]:
            view = root / f"{name}-view.json"
            view.write_text(json.dumps({"layout": "side_by_side_left_right", "eye": "left",
                                       "projection": projection}) + "\n")
            generate(name, root / f"{name}.mp4", ["--video-view", view])
        policy = root / "mixed-policy.json"
        policy.write_text(json.dumps({"intent": "faithful", "mapping": {
            "axis": "stroke", "amplitude": 0.2, "offset": 0.4}}) + "\n")
        mixed_actions = generate("faithful-mixed", mixed, ["--mixed-audio", policy])
        assert mixed_actions == ordinary, "faithful mixed source changed visual actions"
        prompt = "pattern=triangle axis=stroke duration=1s frequency=1hz amplitude=0.1 offset=0.5"
        for name, extra in [("text-pattern", []), ("image-pattern", ["--image", image])]:
            output = root / f"{name}.funscript"
            invoke(["synthesize", prompt, "--output", output, *extra])
            actions = inspect_script(output)
            cases.append({"name": name, "state": "PASS", "actions": len(actions),
                          "output": output.name, "output_identity": identity(output),
                          "scope": "explicit deterministic synthesis; no free-form interpretation claim"})
        output = root / "audio.funscript"
        invoke(["audio-map", root / "clean.m4a", "--output", output,
                "--amplitude", "0.2", "--offset", "0.4"])
        cases.append({"name": "audio-envelope", "state": "PASS", "actions": len(inspect_script(output)),
                      "output": output.name, "output_identity": identity(output),
                      "scope": "full-band RMS synthesis, not beat or semantic interpretation"})
        imported = root / "original.funscript"
        original = {"actions": [{"at": i * 250, "pos": value}
                                for i, value in enumerate([40, 45, 40, 40, 35, 40])]}
        imported.write_text(json.dumps(original) + "\n")
        imported_identity = identity(imported)
        project = ok(["project", "create", "Authored input qualification"])["value"]["project_id"]
        candidate = ok(["import-script", imported, "--project", project])["value"]["candidate_id"]
        ok(["commit-candidate", project, candidate, "--revision", "0"])
        snapshot = ok(["project", "open", project])["value"]
        exported = root / "import-roundtrip.funscript"
        invoke(["export-axis", project, exported])
        assert inspect_script(exported) == original["actions"] and identity(imported) == imported_identity
        cases.append({"name": "existing-script-roundtrip", "state": "PASS", "source_preserved": True,
                      "output": exported.name, "output_identity": identity(exported)})
        for name, command in [
            ("corrupt-audio", ["audio-map", root / "damaged-1.m4a"]),
            ("corrupt-video", ["generate", root / "damaged-1.mp4", "--profile", "fast"]),
            ("custom-fps-refusal", ["generate", root / "clean.mp4", "--fps", "8"]),
            ("free-form-pattern-refusal", ["synthesize", "make an interesting rhythm"]),
            ("missing-image-prompt", ["synthesize", "", "--image", image])]:
            output = root / f"{name}.funscript"
            invoke([*command, "--output", output], success=False)
            assert not output.exists(), "refused input published playback script"
            cases.append({"name": name, "state": "PASS", "refused": True, "output_absent": True})
        assert ok(["project", "open", project])["value"] == snapshot
        engine.terminate()
        engine.wait(timeout=5)
        engine = subprocess.Popen([str(binary), "engine"], env=environment,
                                  stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=engine_log)
        deadline = time.monotonic() + 15
        while True:
            assert engine.poll() is None and time.monotonic() < deadline
            if (state / "engine.sock").exists():
                # A stale socket can survive until the new owned listener is ready.
                try:
                    reopened = ok(["project", "open", project])["value"]
                    break
                except AssertionError:
                    pass
            time.sleep(0.025)
        assert reopened == snapshot and identity(imported) == imported_identity
        cases.append({"name": "script-restart", "state": "PASS", "master_preserved": True})
    finally:
        if engine is not None and engine.poll() is None:
            engine.terminate()
            try:
                engine.wait(timeout=5)
            except subprocess.TimeoutExpired:
                engine.kill()
                engine.wait(timeout=5)
        engine_log.close()
        joined = engine is None or engine.poll() is not None
        (root / "custody.json").write_text(json.dumps({"engine_joined": joined}) + "\n")
    (root / "result.json").write_text(json.dumps({
        "qualified": False, "physical_device_used": False, "authored_media": True,
        "candidate_manifest_identity": identity(runtime / "runtime-candidate.json"),
        "engine_joined": joined, "cases": cases, "cli_commands": len(records),
        "tool_overrides": False, "scope": "current-host offline CLI/component candidate",
        "limitations": ["Not Core or Complete", "No live/device workflow exercised",
                        "No local-AI/free-form interpretation or model quality qualification",
                        "No general codec/media/platform/edition qualification"]
    }, indent=2) + "\n")
    print(f"{len(cases)} authored offline workflow checks passed; qualified=false")


if __name__ == "__main__":
    main()
