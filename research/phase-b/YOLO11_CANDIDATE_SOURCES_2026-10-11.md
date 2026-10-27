# YOLO11n candidate sources — 2026-10-11

Status: source research only. No weights were downloaded, no environment changed, and no export or Pulsar qualification was performed by this research task.

## Candidate and licensing boundary

Ultralytics lists `yolo11n.pt` as a detection model trained on COCO's 80 classes and supports ONNX export. This is a generic object detector; its published class set does not establish the target/view labels or checked script timing Pulsar needs. It can provide a supplier/export/inference probe and potentially a person detector, but cannot be called a useful target-trained generation model from these sources. [Official YOLO11 documentation](https://docs.ultralytics.com/models/yolo11/).

The publisher explicitly offers YOLO11 models under AGPL-3.0 or Enterprise terms and states that its trained models are covered by AGPL-3.0 by default. An AGPL candidate is consistent with the project's stated license direction, subject to corresponding-source, notices, artifact provenance and release review. This note does not give legal approval, authorize a proprietary release, or establish training-data rights. [Publisher licensing page](https://www.ultralytics.com/license), [source license at the inspected revision](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/LICENSE).

## Official weight identity

The [official v8.3.0 release API](https://api.github.com/repos/ultralytics/assets/releases/tags/v8.3.0) returned:

| Field | Observed value |
| --- | --- |
| Release ID | `177482232` |
| Asset name / ID | `yolo11n.pt` / `196201276` |
| Asset size | `5,613,764` bytes |
| Created / updated | `2024-10-01T19:44:21Z` / `2024-10-01T19:44:22Z` |
| API digest | `null` |
| Download | [official release asset](https://github.com/ultralytics/assets/releases/download/v8.3.0/yolo11n.pt) |

The [v8.3.0 annotated tag](https://api.github.com/repos/ultralytics/assets/git/ref/tags/v8.3.0) is `8cca9a538a34f9edcacfa4635029049e4c46044b`, resolving to [commit `0f20d143ac8f9e01322f85dcb5c10dc9b7ca1153`](https://api.github.com/repos/ultralytics/assets/git/tags/8cca9a538a34f9edcacfa4635029049e4c46044b). A source tag does **not** content-pin a replaceable release asset. Since GitHub supplies no digest for this asset, the main probe must record the downloaded SHA-256 and byte length before loading it, alongside this URL and asset metadata. There is no independently verified weight SHA-256 in this note.

## Export contract and exact proposed arguments

Inspected exporter source: Ultralytics commit `69c95d1add13a232382e53b3aa5094ec0c7738db`, reporting version `8.4.175`. Pin this source revision or separately inspect the selected installed release; do not silently apply these details to another version. [Version](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/ultralytics/__init__.py#L3).

For this revision, the proposed CPU FP32 probe is:

```python
from ultralytics import YOLO

# Absolute local path to the already downloaded and SHA-256 recorded asset.
model = YOLO(verified_weights_path, task="detect")
model.export(
    format="onnx", imgsz=640, batch=1, device="cpu",
    dynamic=False, simplify=False, opset=17,
    nms=None, quantize=32,
)
```

Here `nms=None` explicitly requests raw predictions for external NMS. Avoid copying YOLO26's end-to-end settings into a YOLO11 contract. The exporter names the input `images` and detection output `output0`; its documented detection shape is `[1,84,8400]` for 640 square input. The ordinary detection head concatenates decoded boxes and sigmoid class scores along channel dimension 1. Four box channels plus 80 classes and `80²+40²+20²=8400` candidates imply this expected channel-major shape. Confirm actual static FLOAT input `[1,3,640,640]`, output, metadata class order, absence of embedded NMS, decoded coordinate semantics and numerical parity from the generated artifact before importing it into Pulsar. [Exporter](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/ultralytics/engine/exporter.py#L1113), [head](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/ultralytics/nn/modules/head.py#L206), [YOLO11 architecture](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/ultralytics/cfg/models/11/yolo11.yaml).

Preprocessing is a separate integration obligation: RGB order, scaling to `[0,1]`, and the same aspect-preserving letterbox/coordinate reversal as the selected predictor. Tensor dimensions alone do not prove parity. [Official predictor source](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/ultralytics/engine/predictor.py).

## Host dependency boundary

Official PyTorch instructions pair Torch `2.11.0` with torchvision `0.26.0`, including CUDA 13.0 wheels. Preserve the host's Torch installation; use that matching torchvision ABI rather than resolving an arbitrary current version. [Official PyTorch version matrix](https://pytorch.org/get-started/previous-versions/#v2110).

Ultralytics metadata permits Torch `>=1.8.0` and torchvision `>=0.9.0`, but those broad lower bounds do not prove this host pair runs successfully. For the chosen unsimplified FP32 ONNX export, the exporter checks `onnx>=1.12.0,<2.0.0`; simplification adds `onnxslim>=0.1.82` and an ONNX Runtime variant. Its helper explicitly passes `dynamo=False` on modern Torch, so `onnxscript` is not required by this legacy export path. ONNX Runtime is still needed separately for numerical and Pulsar worker inference checks. Record exact installed versions and retain failures; successful import/export must be measured locally. [Dependency metadata](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/pyproject.toml), [ONNX export dependency checks](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/ultralytics/engine/exporter.py#L1113), [Torch export helper](https://github.com/ultralytics/ultralytics/blob/69c95d1add13a232382e53b3aa5094ec0c7738db/ultralytics/utils/export/engine.py#L117).
