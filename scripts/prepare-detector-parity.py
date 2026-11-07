"""Create an opt-in numerical fixture for the pinned official YOLO11 export.

Run offline in an isolated environment. This comparison uses Pulsar's nearest
stretch, not the publisher predictor's letterbox, and makes no quality claim.
"""
import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
import torch
from PIL import Image
from ultralytics import YOLO
from ultralytics.utils.nms import non_max_suppression


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser()
    for name in ("weights", "model", "runtime", "image", "output"):
        p.add_argument("--" + name, required=True, type=Path)
    p.add_argument("--letterbox", action="store_true")
    a = p.parse_args()
    assert all(getattr(a, n).is_absolute() for n in ("weights", "model", "runtime", "image", "output"))
    assert digest(a.weights) == "0ebbc80d4a7680d14987a577cd21342b65ecfd94632bd9a8da63ae6417644ee1"
    assert digest(a.model) == "36849cda776713a29472569fc5955b6015994f1b2f0063ac53427c9b8340fb74"
    assert digest(a.runtime) == "b5c9d4f124d24707f514dad926dc181820807178855df1c528e3addb2dd0e6f7"
    assert digest(a.image) == "c02019c4979c191eb739ddd944445ef408dad5679acab6fd520ef9d434bfbc63"
    torch.set_num_threads(2)
    with Image.open(a.image) as im:
        assert im.width <= 4096 and im.height <= 4096 and im.width * im.height <= 4 * 1024 * 1024
        rgb = np.asarray(im.convert("RGB"))
    h, w, _ = rgb.shape
    # Integer floor coordinates and float32 division match the declared adapter.
    if a.letterbox:
        from ultralytics.data.augment import LetterBox
        resized = LetterBox(new_shape=(640, 640), auto=False, scale_fill=False,
                           scaleup=True, center=True, padding_value=114)(image=rgb)
    else:
        resized = rgb[(np.arange(640) * h // 640)[:, None], (np.arange(640) * w // 640)[None, :]]
    tensor = resized.transpose(2, 0, 1).astype(np.float32)[None] / np.float32(255)
    model = YOLO(str(a.weights), task="detect").model.float().eval().fuse()
    with torch.inference_mode():
        prediction = model(torch.from_numpy(tensor.copy()))[0].cpu()
        assert tuple(prediction.shape) == (1, 84, 8400)
        publisher_detections = non_max_suppression(prediction.clone(), conf_thres=0.25, iou_thres=0.45)[0].tolist()
    assert publisher_detections, "positive-detection fixture required"
    fixture = {"model": str(a.model), "runtime": str(a.runtime), "width": w, "height": h}
    if a.letterbox:
        fixture.update(resize="letterbox", padding_byte=114)
    for name, data in (("rgb", rgb.tobytes()), ("input", tensor.astype("<f4").tobytes()),
                       ("output", prediction.numpy().astype("<f4").tobytes())):
        path = a.output / (name + ".raw")
        with path.open("xb") as f:
            f.write(data)
        fixture[name] = str(path)
        fixture[name + "_sha256"] = digest(path)
    with (a.output / "fixture.json").open("x") as f:
        json.dump(fixture, f, indent=2)
        f.write("\n")
    report = {"qualified": False, "torch": torch.__version__, "input_shape": list(tensor.shape),
              "output_shape": list(prediction.shape), "publisher_nms": publisher_detections,
              "image_source": "https://raw.githubusercontent.com/ultralytics/ultralytics/69c95d1add13a232382e53b3aa5094ec0c7738db/ultralytics/assets/bus.jpg",
              "preprocessing": "RGB NCHW fixed centered linear letterbox, padding 114, float32 division by 255" if a.letterbox else "RGB NCHW integer-floor nearest stretch, float32 division by 255; no letterbox",
              "scope": "same-input numerical comparison only; no script or target quality claim"}
    if a.letterbox:
        ratio = min(640 / w, 640 / h)
        rw, rh = max(1, round(w * ratio)), max(1, round(h * ratio))
        report["transform"] = {"source_dimensions": [w, h], "resized_dimensions": [rw, rh],
                               "padding": [(640 - rw) // 2, (640 - rh) // 2]}
    with (a.output / "publisher.json").open("x") as f:
        json.dump(report, f, indent=2)
        f.write("\n")
    print(json.dumps({"input": tensor.shape, "output": list(prediction.shape), "detections": len(publisher_detections), "qualified": False}))


if __name__ == "__main__":
    main()
