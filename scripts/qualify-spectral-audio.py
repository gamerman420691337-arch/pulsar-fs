#!/usr/bin/env python3
"""Native authored-tone CLI checks; not media, beat, GUI or device qualification."""
import argparse
import hashlib
import json
import math
import struct
import shutil
import subprocess
import time
from pathlib import Path


def identity(path):
    with path.open("rb") as stream:
        return {"sha256": hashlib.file_digest(stream, "sha256").hexdigest(),
                "byte_len": path.stat().st_size}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    parser.add_argument("--ffprobe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    app, ffmpeg, ffprobe = (p.resolve(strict=True) for p in (args.app, args.ffmpeg, args.ffprobe))
    root = args.output.absolute()
    root.mkdir(mode=0o700)
    state = root / "state"
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(root), "TMPDIR": str(root),
                   "PULSAR_STATE_DIR": str(state), "PULSAR_FFMPEG": str(ffmpeg),
                   "PULSAR_FFPROBE": str(ffprobe)}
    records, cases = [], []
    engine = None
    engine_log = (root / "engine.stderr").open("w")

    def invoke(arguments, success=True):
        result = subprocess.run([str(app)] + list(map(str, arguments)), env=environment,
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=45)
        assert len(result.stdout) + len(result.stderr) <= 1024*1024
        record = {"arguments": list(map(str, arguments)), "exit": result.returncode,
                  "stdout": result.stdout, "stderr": result.stderr}
        records.append(record)
        (root / f"cli-{len(records):03}.json").write_text(json.dumps(record, indent=2)+"\n")
        assert (result.returncode == 0) == success, record

    def synth(name, source, extra, expected):
        output = root / f"{name}.funscript"
        if expected == "vibe-map":
            invoke(["audio-map",source,"--output",output,"--mode","vibe","--axis","yaw","--amplitude","0.2","--offset","0.4"])
        else:
            invoke(["audio-synth", source, "--output", output, *extra])
        actions = json.loads(output.read_text())["actions"]
        assert actions and all(isinstance(a["at"], int) and 0 <= a["pos"] <= 100 for a in actions)
        assert all(a["at"] < b["at"] for a,b in zip(actions, actions[1:]))
        assert actions[0]["at"] == 0 and actions[-1]["at"] == 2000
        steady = [a["pos"] for a in actions if 750 <= a["at"] <= 1000]
        if expected == "vibe-raw" or expected == "vibe-map":
            raw=source.with_suffix(".f32").read_bytes()
            pcm=[x[0] for x in struct.iter_unpack("<f",raw)]
            levels=[math.sqrt(sum(x*x for x in pcm[i:i+160])/len(pcm[i:i+160])) for i in range(0,len(pcm),160)]
            levels.append(levels[-1])
            assert len(actions)==len(levels)
            half_span=0.1 if expected == "vibe-map" else 0.25
            for index,(action,level) in enumerate(zip(actions,levels)):
                assert action["at"] == index*10
                # Independent known PCM/RMS and carrier oracle, then neutral quantization.
                predicted=math.floor(100*(0.5+half_span*level*math.sin(math.tau*8*index/100))+0.5)
                assert action["pos"]==predicted,(index,action,predicted)
        elif expected == "vibe-pass":
            assert steady and min(steady)<40 and max(steady)>60,steady
            assert all(35 <= p <= 65 for p in steady),steady
            assert actions[0]["pos"]==50 and actions[-1]["pos"]==50
        elif expected == "vibe-center": assert all(a["pos"]==50 for a in actions),actions
        elif expected == "pass": assert steady and all(49 <= p <= 55 for p in steady), steady
        elif expected == "attenuated":
            assert steady and all(25 <= p <= 30 for p in steady), steady
            assert max(a["pos"] for a in actions) <= 40
        elif expected == "pulse": assert max(a["pos"] for a in actions) == 75
        else: assert all(a["pos"] == 25 for a in actions), actions
        cases.append({"name": name, "state": "PASS", "actions": len(actions), "output": identity(output)})

    evidence = args.evidence.absolute()
    evidence.mkdir(mode=0o700)
    def retain():
        for path in root.iterdir():
            if path.is_file() and path.suffix in [".json", ".funscript", ".stderr"]:
                shutil.copyfile(path, evidence/path.name)
        (evidence/"fixture-identities.json").write_text(json.dumps({p.name:identity(p) for p in root.iterdir() if p.suffix in [".f32", ".wav"]},indent=2)+"\n")

    try:
        for hz in [100, 1000, 5000]:
            raw = root / f"tone-{hz}.f32"
            raw.write_bytes(b"".join(struct.pack("<f", 0.8*math.sin(math.tau*hz*n/16000)
                                                if 4000 <= n < 20000 else 0.0) for n in range(32000)))
            result = subprocess.run([str(ffmpeg), "-v", "error", "-nostdin", "-f", "f32le", "-ar", "16000", "-ac", "1",
                                     "-i", str(raw), "-c:a", "pcm_f32le", str(root/f"tone-{hz}.wav")],
                                    env=environment, stdin=subprocess.DEVNULL, capture_output=True, timeout=20)
            assert result.returncode == 0, result.stderr
        engine = subprocess.Popen([str(app), "engine"], env=environment, stdin=subprocess.DEVNULL,
                                  stdout=subprocess.DEVNULL, stderr=engine_log)
        deadline = time.monotonic()+15
        while not (state / "engine.sock").exists():
            assert engine.poll() is None and time.monotonic() < deadline
            time.sleep(0.025)
        for hz, band in [(100,"bass"),(1000,"mid"),(5000,"high")]:
            for target in ["bass","mid","high"]:
                synth(f"{hz}-{target}",root/f"tone-{hz}.wav",["--mode","envelope","--band",target,"--threshold","0.05"],
                      "pass" if target == band else "attenuated")
        synth("bass-default",root/"tone-100.wav",[],"pulse")
        synth("full-legacy",root/"tone-1000.wav",["--mode","envelope","--band","full"],"pass")
        synth("full-gated",root/"tone-1000.wav",["--mode","envelope","--band","full","--threshold","0.7"],"reject")
        synth("bass-onsets",root/"tone-100.wav",["--band","bass","--threshold","0.2"],"pulse")
        synth("bass-no-onsets",root/"tone-100.wav",["--band","bass","--threshold","0.7"],"reject")
        for mode in ["vibe","osc","oscillation"]:
            synth(f"{mode}-full",root/"tone-1000.wav",["--mode",mode,"--band","full"],"vibe-raw")
        synth("vibe-bass",root/"tone-100.wav",["--mode","vibe","--band","bass","--threshold","0.05"],"vibe-pass")
        synth("vibe-gated",root/"tone-100.wav",["--mode","vibe","--threshold","0.7"],"vibe-center")
        synth("vibe-rejected-band",root/"tone-5000.wav",["--mode","vibe"],"vibe-center")
        synth("vibe-map-yaw",root/"tone-1000.wav",[],"vibe-map")
        for extra in [["--band","unknown"],["--threshold","0"],["--threshold","NaN"],["--bundle"]]:
            output=root/"refused.funscript"
            invoke(["audio-synth",root/"tone-100.wav","--output",output,*extra],success=False)
            assert not output.exists()
    finally:
        if engine is not None and engine.poll() is None:
            engine.terminate()
            try: engine.wait(timeout=5)
            except subprocess.TimeoutExpired:
                engine.kill(); engine.wait(timeout=5)
        engine_log.close()
        (root/"custody.json").write_text(json.dumps({"engine_joined":engine is None or engine.poll() is not None})+"\n")
        retain()
    (root/"result.json").write_text(json.dumps({"qualified":False,"physical_device_used":False,
        "app":identity(app),"ffmpeg":identity(ffmpeg),"ffprobe":identity(ffprobe),
        "cases":cases,"commands":len(records),"refusals":4,"engine_joined":True},indent=2)+"\n")

    retain()


if __name__ == "__main__":
    main()
