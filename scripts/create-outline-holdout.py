#!/usr/bin/env python3
"""Generate labelled synthetic clips for independent outline-tracker evaluation."""

import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
import tempfile

from PIL import Image, ImageDraw


FPS = 12
FRAMES = 144
SIZE = 256
CASES = ("clean", "clutter", "low-contrast")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def endpoint(index):
    phase = 2 * math.pi * index / (FPS * 2.5)
    return (
        round(186 + 16 * math.sin(phase / 2 + 0.7)),
        round(126 + 65 * math.sin(phase) + 17 * math.sin(phase * 2 + 0.4)),
    )


def frame(index, case):
    x, y = endpoint(index)
    background = 95 if case == "low-contrast" else 24
    foreground = 143 if case == "low-contrast" else 232
    image = Image.new("RGB", (SIZE, SIZE), (background,) * 3)
    draw = ImageDraw.Draw(image)
    if case == "clutter":
        decoy_y = round(130 + 77 * math.cos(index * 0.19))
        draw.ellipse((38, decoy_y - 13, 64, decoy_y + 13), outline=(190,) * 3, width=3)
        draw.line((18, 220, 117, 220), fill=(124,) * 3, width=3)
    # The terminal vertex is the declared target. The outline uses a different
    # silhouette from the training generator and contains no reference frames.
    tail_y = y + round(28 * math.cos(index * 0.1))
    shape = [(22, tail_y - 11), (x - 44, y - 13), (x - 18, y - 21),
             (x, y), (x - 18, y + 21), (x - 44, y + 13), (22, tail_y + 11)]
    draw.line([*shape, shape[0]], fill=(foreground,) * 3, width=5, joint="curve")
    return image.tobytes()


def build_case(output, case):
    clip = output / f"{case}.mp4"
    labels = output / f"{case}.funscript"
    with tempfile.TemporaryFile(dir=output) as raw:
        for index in range(FRAMES):
            raw.write(frame(index, case))
        raw.seek(0)
        subprocess.run(
            ["ffmpeg", "-nostdin", "-loglevel", "error", "-f", "rawvideo", "-pixel_format",
             "rgb24", "-video_size", f"{SIZE}x{SIZE}", "-framerate", str(FPS), "-i", "pipe:0",
             "-c:v", "libx264", "-threads", "1", "-pix_fmt", "yuv420p", str(clip)],
            stdin=raw, timeout=30, check=True,
        )
    actions = [
        {"at": round(index * 1000 / FPS), "pos": round(100 * (1 - endpoint(index)[1] / (SIZE - 1)))}
        for index in range(FRAMES)
    ]
    labels.write_text(json.dumps({"version": "1.0", "inverted": False, "range": 100,
                                  "actions": actions}, indent=2) + "\n")
    return {"case": case, "clip": clip.name, "clip_sha256": digest(clip),
            "reference": labels.name, "reference_sha256": digest(labels)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="New directory for the generated holdout")
    output = parser.parse_args().output
    output.mkdir(mode=0o700)
    cases = [build_case(output, case) for case in CASES]
    manifest = {"origin": "independently generated geometry", "fps": FPS, "frames": FRAMES,
                "playback_offset_ms": 0, "cases": cases}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
