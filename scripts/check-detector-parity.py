"""Compare the Rust campaign report with the publisher's independent NMS.

This is a test utility, not an Engine admission or model qualification path.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path


def bounded(path):
    assert path.is_absolute()
    with path.open("rb") as f:
        data = f.read(4 * 1024 * 1024 + 1)
    assert len(data) <= 4 * 1024 * 1024
    return data


def main():
    p = argparse.ArgumentParser()
    for name in ("rust-log", "publisher", "output"):
        p.add_argument("--" + name, required=True, type=Path)
    a = p.parse_args()
    log, reference = bounded(a.rust_log), bounded(a.publisher)
    reports = [json.loads(line.split("=", 1)[1]) for line in log.decode().splitlines()
               if line.startswith("comparison_json=")]
    assert len(reports) == 1
    actual, publisher = reports[0], json.loads(reference)
    assert actual["qualified"] is False and publisher["qualified"] is False
    assert actual["input_bit_identical"] is True
    assert actual["raw_coordinate_max_absolute_error"] <= 0.03
    assert actual["raw_score_max_absolute_error"] <= 0.0001
    expected = publisher["publisher_nms"]
    assert expected and len(actual["detections"]) == len(expected)
    maximum_box_error, maximum_score_error = 0, 0
    for got, row in zip(actual["detections"], expected):
        assert len(row) == 6 and all(math.isfinite(v) for v in row)
        assert len(got["bbox"]) == 4 and all(math.isfinite(v) and 0 <= v <= 1 for v in got["bbox"])
        assert math.isfinite(got["score"]) and 0 <= got["score"] <= 1
        assert type(got["class_id"]) is int and 0 <= got["class_id"] < 80
        assert got["class_id"] == row[5]
        # Publisher NMS precedes clipping; compare its selected boxes after
        # clipping to the declared square model-space bounds and normalization.
        if "transform" in publisher:
            transform = publisher["transform"]
            dimensions = transform["resized_dimensions"]
            padding = transform["padding"]
            assert len(dimensions) == 2 and all(0 < v <= 640 for v in dimensions)
            assert len(padding) == 2 and all(0 <= v <= 640 for v in padding)
            box = [min(1, max(0, (v - padding[i % 2]) / dimensions[i % 2])) for i, v in enumerate(row[:4])]
        else:
            box = [min(640, max(0, v)) / 640 for v in row[:4]]
        maximum_box_error = max(maximum_box_error, *(abs(v - w) for v, w in zip(got["bbox"], box)))
        maximum_score_error = max(maximum_score_error, abs(got["score"] - row[4]))
    assert maximum_box_error <= 0.0001 and maximum_score_error <= 0.0001
    report = {"result": "PASS", "qualified": False, "detections": len(expected),
              "maximum_normalized_box_error": maximum_box_error,
              "maximum_score_error": maximum_score_error,
              "rust_log_sha256": hashlib.sha256(log).hexdigest(),
              "publisher_report_sha256": hashlib.sha256(reference).hexdigest(),
              "scope": "one pinned non-square image, same declared tensor and inverse mapping; no model quality or script qualification"}
    assert a.output.is_absolute()
    with a.output.open("x") as f:
        json.dump(report, f, indent=2)
        f.write("\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
