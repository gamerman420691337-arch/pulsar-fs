#!/usr/bin/env python3
"""Prepare a private Browser live project for the supplied sterile clip."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("pulsar", "clip", "model", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument(
        "--launch", action="store_true",
        help="Serve the prepared Browser Live player with the same Engine environment",
    )
    args = parser.parse_args()
    app, clip, model = (path.resolve(strict=True) for path in (args.pulsar, args.clip, args.model))
    expected_source = "71d381468f83fd6e4116bb5d98b1c4453431702d1444e90b3c16d9d6d318494f"
    with clip.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != expected_source:
            parser.error("expected the original 14.25-second sterile clip")
    output = args.output.resolve()
    output.mkdir(mode=0o700)
    env = dict(os.environ, PULSAR_STATE_DIR=str(output / "state"))
    for key in ("HANDY_API_KEY", "HANDY_CONNECTION_KEY"):
        env.pop(key, None)

    def run(*command):
        return json.loads(subprocess.check_output(
            [str(app), *map(str, command)], env=env, stdin=subprocess.DEVNULL, timeout=30,
        ))

    profile = run("model", "profile", model, "--width", 64, "--height", 64,
                  "--class-count", 1, "--temporal-script-position")
    probe = run("info", clip)
    if probe["source_version"] != expected_source:
        raise ValueError("Engine source differs from the checked sterile clip")
    zero = {"numerator": 0, "denominator": 1}
    placement = output / "placement.json"
    placement.write_text(json.dumps({
        "source_version": probe["source_version"],
        "source_start": zero, "source_end": {"numerator": 57, "denominator": 4},
        "source_anchor": zero, "project_anchor": 0,
        "rate_numerator": 1, "rate_denominator": 1,
    }))
    project = probe["project_id"]
    revision = run("project", "place", project, "--spec", placement)["value"]["revision"]
    options = output / "options.json"
    options.write_text(json.dumps({
        "inference": {"model_path": str(model), "profile": profile},
        "policy": {"maximum_analysis_age_ns": 1_000_000_000, "added_buffer_ns": 0},
        "source_anchor": zero, "project_anchor": 0, "presentation_anchor": 0,
        "lease_ms": 30_000,
    }, indent=2) + "\n")
    command = [str(app), "browser-live", project, str(revision), str(options)]
    if args.launch:
        os.execve(app, command, env)
    print(json.dumps({"state_dir": env["PULSAR_STATE_DIR"], "command": command}))


if __name__ == "__main__":
    main()
