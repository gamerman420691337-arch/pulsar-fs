#!/usr/bin/env python3
"""Author bounded codec controls; results do not qualify real media or devices."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path


def run(arguments):
    return subprocess.run(list(map(str, arguments)), stdin=subprocess.DEVNULL,
                          capture_output=True, timeout=30)


def identity(path):
    data = path.read_bytes()
    return {"sha256": hashlib.sha256(data).hexdigest(), "byte_len": len(data)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    parser.add_argument("--ffprobe", type=Path, required=True)
    args = parser.parse_args()
    root = args.output.absolute()
    ffmpeg = args.ffmpeg.resolve(strict=True)
    ffprobe = args.ffprobe.resolve(strict=True)
    root.mkdir(mode=0o700)
    rows = []
    files = []
    for codec, extension in [("flac", "flac"), ("mp2", "mp2"), ("aac", "m4a")]:
        clean = root / ("clean." + extension)
        encoded = run([ffmpeg, "-v", "error", "-nostdin", "-f", "lavfi", "-i",
                       "sine=frequency=440:sample_rate=16000:duration=2",
                       "-threads", "1", "-c:a", codec, clean])
        if encoded.returncode:
            raise RuntimeError(f"Authored {codec} encoding failed: {encoded.stderr.decode(errors='replace')}")
        files.append(clean)
        packet_probe = run([ffprobe, "-v", "error", "-select_streams", "a:0",
                            "-show_packets", "-show_entries", "packet=pos,size", "-of", "json", clean])
        if packet_probe.returncode:
            raise RuntimeError("Packet probe failed")
        packets = json.loads(packet_probe.stdout)["packets"]
        original = clean.read_bytes()
        for ordinal in [1, 3, 7]:
            packet = packets[ordinal]
            position, size = int(packet["pos"]), int(packet["size"])
            offset, length = min(16, size // 8), size // 2
            if not (0 <= position < len(original) and 0 < length and offset + length <= size
                    and position + size <= len(original)):
                raise RuntimeError("Packet mutation outside authored source")
            data = bytearray(original)
            data[position + offset:position + offset + length] = bytes(length)
            damaged = root / f"damaged-{ordinal}.{extension}"
            damaged.write_bytes(data)
            files.append(damaged)
            metadata = run([ffprobe, "-v", "error", "-threads", "1", "-select_streams", "a:0",
                            "-show_entries", "stream=sample_rate,time_base:frame=best_effort_timestamp,nb_samples,sample_rate",
                            "-show_frames", "-of", "json", damaged])
            (root / f"meta-{ordinal}-{extension}.json").write_bytes(metadata.stdout)
            (root / f"meta-{ordinal}-{extension}.stderr").write_bytes(metadata.stderr)
            row = {"codec": codec, "ordinal": ordinal, "pos": position, "size": size,
                   "mutation_start": position + offset, "mutation_bytes": length,
                   "metadata_exit": metadata.returncode, "metadata_stderr": len(metadata.stderr)}
            for strict in [False, True]:
                command = [ffmpeg, "-v", "error", "-nostdin"]
                if strict:
                    command.append("-xerror")
                command += ["-i", damaged, "-map", "0:a:0", "-vn", "-sn", "-dn", "-ac", "1",
                            "-ar", "16000", "-threads", "1", "-f", "f32le", "-c:a", "pcm_f32le", "pipe:1"]
                decoded = run(command)
                (root / f"decode-{ordinal}-{extension}-{strict}.stderr").write_bytes(decoded.stderr)
                row[str(strict)] = {"exit": decoded.returncode, "samples": len(decoded.stdout) // 4,
                                    "stderr": len(decoded.stderr), "output_sha256": hashlib.sha256(decoded.stdout).hexdigest()}
            rows.append(row)
    (root / "observations.json").write_text(json.dumps(rows, indent=2) + "\n")
    affected = [row for row in rows if row["codec"] in ("flac", "aac")]
    if not all(row["False"]["exit"] == 0 and row["False"]["stderr"] > 0
               and row["True"]["exit"] != 0 for row in affected):
        raise RuntimeError("Codec concealment controls did not reproduce")
    (root / "subjects.json").write_text(json.dumps({
        "qualified": False, "physical_device_used": False, "authored_synthetic_media": True,
        "tools": {str(tool): identity(tool) for tool in [ffmpeg, ffprobe]},
        "files": {path.name: identity(path) for path in files},
        "limitation": "MP2 mutations emit no codec error; strict exit handling does not detect arbitrary byte corruption"
    }, indent=2) + "\n")
    print("Nine authored audio mutation controls recorded; qualified=false")


if __name__ == "__main__":
    main()
