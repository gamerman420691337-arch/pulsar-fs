#!/usr/bin/env python3
"""Bounded request-withdrawal model and specific negative mutation controls."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

PIN = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--scratch-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=120)
    args = parser.parse_args()
    if not 1 <= args.timeout <= 600:
        parser.error("timeout must be 1..600 seconds")
    jar = args.jar.resolve(strict=True)
    if not jar.is_file() or jar.stat().st_size > 16 * 1024 * 1024:
        parser.error("TLC must be the bounded pinned release jar")
    if hashlib.sha256(jar.read_bytes()).hexdigest() != PIN:
        parser.error("TLC differs from the recorded Phase A pin")
    root = args.scratch_root.resolve(strict=True)
    if not root.is_dir():
        parser.error("scratch-root must be an existing directory")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    here = Path(__file__).resolve().parent
    source = (here / "WasmStartResolution.tla").read_text()
    config = (here / "WasmStartResolution.cfg").read_text()
    mutations = [
        ("withdrawal-fence", '  /\\ namespace[a].kind = "Free"\n', "", "NoStartAfterWithdrawal"),
        ("accepted-reference", ' /\\ namespace[a].reference = ref)', ')', "ResolutionCorrespondence"),
        ("resolve-authority", 'Resolve(a, ref) ==\n  /\\ Authorized(a)\n', 'Resolve(a, ref) ==\n', "NoUnauthorizedResolution"),
        ("replay-authority", 'Replay(a, ref) ==\n  /\\ Authorized(a)\n', 'Replay(a, ref) ==\n', "NoUnauthorizedReplay"),
    ]
    results = []
    with tempfile.TemporaryDirectory(prefix="wasm-resolution-model-", dir=root) as work:
        work = Path(work)
        cases = [("positive", source, None)]
        for name, target, replacement, invariant in mutations:
            if source.count(target) != 1:
                raise RuntimeError(f"mutation target changed: {name}")
            cases.append((name, source.replace(target, replacement), invariant))
        for name, model, invariant in cases:
            case = work / name
            case.mkdir()
            (case / "WasmStartResolution.tla").write_text(model)
            (case / "WasmStartResolution.cfg").write_text(config)
            command = ["java", "-XX:+UseParallelGC", f"-Djava.io.tmpdir={case}",
                       "-cp", str(jar), "tlc2.TLC", "-workers", "1",
                       "-config", "WasmStartResolution.cfg", "WasmStartResolution"]
            with (output / f"{name}.log").open("w") as log:
                try:
                    process = subprocess.run(command, cwd=case, stdout=log,
                                             stderr=subprocess.STDOUT, timeout=args.timeout)
                    code = process.returncode
                except subprocess.TimeoutExpired:
                    code = 124
            text = (output / f"{name}.log").read_text()
            passed = (code == 0 and "Model checking completed. No error has been found." in text
                      if invariant is None else
                      code == 12 and f"Invariant {invariant} is violated" in text)
            results.append({"case": name, "exit_code": code,
                            "expected_invariant": invariant, "passed": passed})
            if not passed:
                break
    (output / "results.json").write_text(json.dumps({"tlc_sha256": PIN,
        "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
        "config_sha256": hashlib.sha256(config.encode()).hexdigest(),
        "cases": results, "scope": "two actors, one original ID each, two metadata references; safety only"}, indent=2) + "\n")
    success = len(results) == 5 and all(result["passed"] for result in results)
    print("PASS" if success else "FAIL", "bounded WASM start resolution and four adequacy controls")
    return 0 if success else 1


if __name__ == "__main__":
    raise SystemExit(main())
