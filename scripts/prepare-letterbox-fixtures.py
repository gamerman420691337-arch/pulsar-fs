"""Independent pinned OpenCV/Ultralytics byte-space resize correspondence cases."""
import argparse
import hashlib
import json
from pathlib import Path

import cv2
import numpy as np
import ultralytics
from ultralytics.data.augment import LetterBox


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--output", required=True, type=Path)
    a = p.parse_args()
    assert a.output.is_absolute() and a.output.is_dir()
    assert cv2.__version__ == "4.13.0" and ultralytics.__version__ == "8.4.175"
    cases = []
    for i, (w, h, tw, th) in enumerate([(810, 1080, 640, 640), (17, 29, 32, 48),
                                      (29, 17, 48, 32), (1, 7, 32, 32), (7, 1, 32, 32),
                                      (1, 1, 13, 9), (3, 2, 2, 3), (8, 5, 5, 8),
                                      (4, 4, 2, 2), (2, 2, 5, 5), (5, 5, 2, 2),
                                      (5, 3, 5, 8), (1920, 1080, 640, 640)]):
        y, x, c = np.indices((h, w, 3))
        rgb = ((x * 37 + y * 71 + c * 113 + (x ^ y) * 19) % 256).astype(np.uint8)
        image = LetterBox(new_shape=(th, tw), auto=False, scale_fill=False, scaleup=True,
                          center=True, padding_value=114)(image=rgb)
        tensor = image.transpose(2, 0, 1).astype(np.float32)[None] / np.float32(255)
        case = {"width": w, "height": h, "target_width": tw, "target_height": th}
        for name, data in (("rgb", rgb.tobytes()), ("input", tensor.astype("<f4").tobytes())):
            path = a.output / f"case-{i}-{name}.raw"
            with path.open("xb") as f:
                f.write(data)
            case[name] = str(path)
            case[name + "_sha256"] = hashlib.sha256(data).hexdigest()
        cases.append(case)
    with (a.output / "interpolation.json").open("x") as f:
        json.dump(cases, f, indent=2)
        f.write("\n")
    print(json.dumps({"cases": len(cases), "opencv": cv2.__version__,
                      "ultralytics": ultralytics.__version__, "qualified": False}))


if __name__ == "__main__":
    main()
