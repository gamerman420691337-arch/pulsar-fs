#!/usr/bin/env python3
"""Authored native audio-bundle controls, not media/device qualification.

Run inside a network-disabled namespace with private RAM TMPDIR. Retain evidence
outside that mount. Never connect to an existing Engine or actuate a device.
"""
import argparse
import hashlib
import json
import math
import os
import re
import sqlite3
import struct
import subprocess
import time
import wave
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--baseline", action="store_true")
    args = parser.parse_args()
    app = args.app.resolve(strict=True)
    root = args.output
    root.mkdir(mode=0o700)
    evidence = args.evidence
    evidence.mkdir(mode=0o700)
    state = root / "state"
    env = dict(os.environ, HOME=str(root), TMPDIR=str(root), PULSAR_STATE_DIR=str(state))
    env.pop("PULSAR_ASSISTANT_SELECTION", None)
    source = root / "authored.wav"
    # Exactly representable PCM16: silence, 100 Hz at 0.4 amplitude, silence.
    samples = [round(0.4 * 32767 * math.sin(math.tau * 100 * i / 16000))
               if 4800 <= i < 24000 else 0 for i in range(32000)]
    with wave.open(str(source), "wb") as stream:
        stream.setnchannels(1)
        stream.setsampwidth(2)
        stream.setframerate(16000)
        stream.writeframes(struct.pack("<" + "h" * len(samples), *samples))
    source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    modes = {"stroke": "envelope", "surge": "envelope", "sway": "vibe",
             "roll": "envelope", "pitch": "beats", "yaw": "envelope"}
    bands = {"stroke": "full", "surge": "bass", "sway": "full",
             "roll": "high", "pitch": "full", "yaw": "mid"}
    recipe = {"channels": [{"mapping": {"axis": axis, "amplitude": 0.2, "offset": 0.4},
                            "mode": mode, "analysis": {"band": bands[axis], "threshold": 0.05}}
                           for axis, mode in modes.items()]}
    recipe_path = root / "recipe.json"
    recipe_path.write_text(json.dumps(recipe))
    records = []

    def call(name, arguments, success):
        result = subprocess.run([str(app)] + list(map(str, arguments)), env=env,
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=90)
        assert len(result.stdout) + len(result.stderr) < 1024 * 1024
        record = {"case": name, "arguments": list(map(str, arguments)), "exit": result.returncode,
                  "stdout": result.stdout, "stderr": result.stderr}
        records.append(record)
        (evidence / f"{name}.json").write_text(json.dumps(record, indent=2))
        assert (result.returncode == 0) == success, record
        return result

    def counts():
        with sqlite3.connect(f"file:{state / 'projects.sqlite3'}?mode=ro", uri=True) as db:
            return {table: db.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
                    for table in ["projects", "jobs", "candidates"]}

    with (evidence / "engine.log").open("w") as log:
        engine = subprocess.Popen([str(app), "engine"], env=env, stdin=subprocess.DEVNULL,
                                  stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 20
            while not (state / "engine.sock").exists():
                assert engine.poll() is None and time.monotonic() < deadline, "owned Engine startup failed"
                time.sleep(0.05)
            if args.baseline:
                result = call("bundle-refused", ["audio-synth", source, "--bundle",
                                                "--output", root / "bundle"], False)
                assert "Unsupported" in result.stderr
                assert counts() == {"projects": 0, "jobs": 0, "candidates": 0}
            else:
                bundle = root / "bundle"
                arguments = ["audio-synth", source, "--bundle", "--bundle-recipe", recipe_path,
                             "--output", bundle]
                result = call("bundle", arguments, True)
                report = json.loads(result.stdout)
                assert report["qualified"] is False and len(report["files"]) == 6
                assert counts() == {"projects": 1, "jobs": 1, "candidates": 1}
                levels = [math.sqrt(sum((x / 32768) ** 2 for x in samples[i:i+160]) / 160)
                          for i in range(0, 32000, 160)]
                levels.append(levels[-1])
                for axis in modes:
                    path = bundle / f"{axis}.funscript"
                    script = json.loads(path.read_text())
                    actions = script["actions"]
                    assert actions[0]["at"] == 0 and actions[-1]["at"] == 2000
                    assert all(a["at"] < b["at"] for a, b in zip(actions, actions[1:]))
                    assert all(0 <= a["pos"] <= 100 for a in actions)
                    if axis in ["stroke", "sway"]:
                        assert len(actions) == 201
                        for i, (action, energy) in enumerate(zip(actions, levels)):
                            energy = energy if energy >= 0.05 else 0
                            position = (0.4 + 0.2 * energy if axis == "stroke"
                                        else 0.5 + 0.1 * energy * math.sin(math.tau * 8 * i / 100))
                            assert action == {"at": i * 10, "pos": math.floor(100 * position + 0.5)}
                    elif axis in ["roll", "yaw"]:
                        assert all(a["pos"] == 40 for a in actions)
                    elif axis == "pitch":
                        assert actions == [{"at": 0, "pos": 40}, {"at": 250, "pos": 40},
                                           {"at": 300, "pos": 60}, {"at": 400, "pos": 40},
                                           {"at": 2000, "pos": 40}]
                    else:
                        assert any(a["pos"] >= 45 for a in actions)
                    (evidence / path.name).write_bytes(path.read_bytes())
                current = counts()
                original_outputs = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in bundle.iterdir()}
                call("existing-directory", arguments, False)
                assert counts() == current
                assert original_outputs == {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in bundle.iterdir()}
                call("missing-recipe", ["audio-synth", source, "--bundle", "--output", root / "missing"], False)
                assert counts() == current and not (root / "missing").exists()
                invalid = root / "invalid.json"
                bad = json.loads(json.dumps(recipe))
                bad["channels"][5] = bad["channels"][0]
                invalid.write_text(json.dumps(bad))
                call("duplicate-axis", ["audio-synth", source, "--bundle", "--bundle-recipe", invalid,
                                        "--output", root / "duplicate"], False)
                call("ambiguous-controls", arguments + ["--band", "high"], False)
                assert counts() == current
                # Occupy a leaf after the command reserves its directory. The
                # existing file must survive, completed exports must remain,
                # and the committed master must support explicit recovery.
                partial = root / "partial"
                fault_arguments = arguments[:-1] + [partial]
                process = subprocess.Popen([str(app)] + list(map(str, fault_arguments)), env=env,
                                           stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    deadline = time.monotonic() + 5
                    while not partial.is_dir():
                        assert process.poll() is None and time.monotonic() < deadline, "publication fault barrier not reached"
                        time.sleep(0.001)
                    blocker = partial / "roll.funscript"
                    with blocker.open("xb") as stream:
                        stream.write(b"retained authored obstruction\n")
                    stdout, stderr = process.communicate(timeout=90)
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait(timeout=10)
                record = {"arguments": list(map(str, fault_arguments)), "exit": process.returncode,
                          "stdout": stdout.decode(), "stderr": stderr.decode(), "fault": "existing roll leaf after directory reservation"}
                (evidence / "partial-publication.json").write_text(json.dumps(record, indent=2))
                assert process.returncode != 0 and "publication incomplete" in record["stderr"]
                assert blocker.read_bytes() == b"retained authored obstruction\n"
                assert (partial / "stroke.funscript").is_file()
                assert not stdout.strip(), "partial publication must not print a success summary"
                project_id = re.search(r"Audio bundle project retained: ([^. ]+)\.", record["stderr"]).group(1)
                recovered = root / "recovered-roll.funscript"
                call("recover-roll", ["export-axis", project_id, recovered, "--axis", "roll"], True)
                assert recovered.read_bytes() == (bundle / "roll.funscript").read_bytes()
                assert counts() == {"projects": 2, "jobs": 2, "candidates": 2}
                (evidence / "recovered-roll.funscript").write_bytes(recovered.read_bytes())
                with sqlite3.connect(f"file:{state / 'projects.sqlite3'}?mode=ro", uri=True) as db:
                    manifest, job_state = db.execute("SELECT manifest,state FROM jobs ORDER BY rowid LIMIT 1").fetchone()
                    assert job_state == "completed"
                    manifest = json.loads(manifest)
                    (evidence / "manifest.json").write_text(json.dumps(manifest, indent=2))
                receipts = []
                for path in state.rglob("*.json"):
                    value = json.loads(path.read_text())
                    if isinstance(value, dict) and value.get("job_id") == manifest["job_id"] and value.get("recipe") == "explicit-six-axis-audio-v1":
                        receipts.append(value)
                assert len(receipts) == 1
                assert receipts[0]["details"]["decode_count"] == 1
                (evidence / "lineage.json").write_text(json.dumps(receipts[0], indent=2))
            assert hashlib.sha256(source.read_bytes()).hexdigest() == source_hash
            (evidence / "counts.json").write_text(json.dumps(counts()))
            (evidence / "recipe.json").write_text(json.dumps(recipe, indent=2))
            (evidence / "source-identity.json").write_text(json.dumps({"sha256": source_hash, "unchanged": True}))
        finally:
            engine.terminate()
            try:
                engine.wait(timeout=15)
            except subprocess.TimeoutExpired:
                engine.kill()
                engine.wait(timeout=10)
            (evidence / "retirement.json").write_text(json.dumps({"pid": engine.pid, "joined": True, "exit": engine.returncode}))
    print("audio bundle authored campaign passed", flush=True)


if __name__ == "__main__":
    main()
