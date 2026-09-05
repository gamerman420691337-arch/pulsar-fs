# Funscript Hub (`fs-hub`)

The unified, open-source cleanroom suite for automated funscript generation, neural motion tracking, interactive timeline editing, and hardware script auditing.

`Funscript-Hub` provides a modern, high-performance desktop interface and unified CLI accessible via the **`fs-hub`** command.

---

## Features

- **Unified Desktop Interface (`fs-hub`)**:
  - 📑 **Timeline Editor**: Immediate-mode interactive canvas (`egui`), zoom/pan across hours of video, keyframe selection and drag editing, playhead scrubbing, simulated playback controller (0.5x, 1x, 1.5x, 2x), and visual speed limit warnings ($> 450 \text{ units/s}$).
  - ⚡ **Generator Studio**: Launch AI Neural YOLOv12 tracking or multi-threaded Lucas-Kanade optical flow generation with real-time progress and live video metadata inspection.
  - 🩺 **Script Doctor & Repair**: Hardware certification linter, motor speed violation detection, timing inversion fixes, micro-jitter removal, and one-click batch repairs.
  - 🔌 **Device & Sync Telemetry**: Real-time T-Code v0.2/v0.3 protocol stream monitor, live vertical stroke position gauge, and UDP network synchronization (`127.0.0.1:8888`) for OSR, Handy, Keon, and MultiFunPlayer.
- **AI Neural YOLO Tracking**:
  - Ultralytics YOLOv12n inference pipeline using ONNX Runtime (`ort`).
  - Detects 10 anatomical classes (`penis`, `glans`, `pussy`, `butt`, `anus`, etc.) with Non-Maximum Suppression (NMS).
  - Organ penetration distance tracking with automatic hybrid optical-flow velocity fallback during occlusions.
- **Multi-Threaded Optical Flow Engine**:
  - Pure Rust data-parallel gradient flow powered by `rayon`.
  - Zero-C-binding FFmpeg streaming (`-hwaccel auto`) for AV1, HEVC, H.264, VP9.
  - 2D velocity divergence ($\nabla \cdot \vec{v}$) interaction tracking, POV anchoring, and VR 180 stereo cropping.
- **Legacy Python Engine (`FunscriptFlow.pyw`)**:
  - Modernized Python implementation with dual photometric cut detection and automated pytest suite.

---

## Installation & Quick Start

### Quick Install (Linux / macOS)
Clone the repository and run the automated installer:
```bash
git clone https://github.com/gamerman420691337-arch/Funscript-Hub.git
cd Funscript-Hub
./install.sh
```
This builds the release binary and symlinks `fs-hub` into `~/.local/bin/fs-hub`.

### Open the Desktop GUI
Simply run from any terminal:
```bash
fs-hub
```
or
```bash
fs-hub gui
```

---

## CLI Usage

### 1. Audit a Funscript with Script Doctor
```bash
fs-hub doctor video.funscript
```

### 2. Automatically Repair a Funscript
Clamps motor speed violations to 450 u/s, removes micro-jitters, sorts, and deduplicates:
```bash
fs-hub fix input.funscript -o output.fixed.funscript
```

### 3. Generate from Video (AI Neural Mode)
```bash
fs-hub generate video.mp4 \
  --model path/to/FunGen-12n-pov-1.1.0.onnx \
  --conf 0.35 \
  --fps 30.0 \
  --pov
```

### 4. Generate from Video (Optical Flow Mode)
```bash
fs-hub generate video.mp4 --fps 30.0 --pov
```

### 5. Probe Video Metadata
```bash
fs-hub info video.mp4
```

---

## Repository Structure

- `open-fungen/`: Native Rust core engine, neural ONNX pipeline, and `egui` desktop application (`fs-hub`).
- `FunscriptFlow.pyw`: Modernized Python desktop batch processor.
- `test_funscript_flow.py`: Comprehensive test suite for the Python engine.
- `install.sh`: One-step release builder and PATH symlink installer.
- `Cargo.toml`: Root virtual workspace configuration.

---

## Running Tests

Run the full Rust test suite across all modules:
```bash
cargo test
```

Run Python unit tests:
```bash
pytest test_funscript_flow.py
```

---

## License
- `open-fungen` / `fs-hub`: Dual-licensed under [Apache-2.0](open-fungen/LICENSE-APACHE) and [MIT](open-fungen/LICENSE-MIT).
- `FunscriptFlow`: Licensed under [Apache-2.0](LICENSE).
