<p align="center">
  <img src="Logo.png" alt="Funscript Hub" width="450">
</p>

# Funscript-Hub

The open-source suite of high-performance tools for automated funscript generation, motion tracking, and script auditing.

`Funscript-Hub` brings together:
1. **[Open-FunGen (`open-fungen/`)](#1-open-fungen-rust-cleanroom-engine)**: A high-performance native **Rust** cleanroom motion tracking engine and **Script Doctor** linter.
2. **[FunscriptFlow (`FunscriptFlow.pyw`)](#2-funscriptflow-modernized-python)**: The modernized, bug-fixed, cross-platform Python GUI & batch processor with automated test coverage.

---

## 1. Open-FunGen (Rust Cleanroom Engine)

Located in [`open-fungen/`](open-fungen/).

- **Blazing Fast**: Pure Rust multi-threaded execution utilizing `rayon` for data-parallel optical flow.
- **Robust Video Ingestion**: Streams raw frames directly from hardware-accelerated FFmpeg (`-hwaccel auto`), handling AV1, HEVC, H.264, and VP9 without C-binding ABI brittleness.
- **Adaptive Divergence Tracking**: Calculates dynamic 2D divergence field ($\nabla \cdot \vec{v}$) with temporal median filtering to lock onto stroke interaction centers.
- **Integrated Script Doctor**: Audits funscripts for motor speed limit violations ($> 450 \text{ units/s}$), timing inversions, and micro-jitter noise.

### Quick Start (Rust)
```bash
cd open-fungen
cargo build --release

# Generate funscript from video
./target/release/open-fungen generate video.mp4

# Audit an existing funscript
./target/release/open-fungen doctor video.funscript

# Probe video metadata
./target/release/open-fungen info video.mp4
```

See [`open-fungen/README.md`](open-fungen/README.md) for full CLI options and documentation.

---

## 2. FunscriptFlow (Modernized Python)

The Python desktop application and batch processor.

### Fixes & Improvements in this Version:
- **Scene Cut Detection**: Fixed dead cut detection with photometric $L_1$ mean difference and polar flow magnitude thresholding.
- **Consistent Target FPS**: Unified downsample frame stepping (default 30 FPS) across all window intervals and detrending filters.
- **Outlier Filtering**: True sliding-window temporal median filtering for motion centers.
- **Cross-Platform**: Seamless support for Linux (`xdg-open`), macOS (`open`), and Windows (`os.startfile`).
- **Worker Optimization**: Eliminated per-chunk process pool churn with persistent execution.
- **Comprehensive Test Suite**: Automated verification via `pytest`.

### Running from Source
```bash
# Using the automated launcher (creates virtualenv and installs dependencies):
./run_funscript_flow.sh --gui

# Or run directly:
python3 FunscriptFlow.pyw
```

### Running Tests
```bash
pytest test_funscript_flow.py
```

---

## How It Works

1. **Optical Flow**: Computes dense vector motion between consecutive frames.
2. **Motion Center Estimation**: Finds the point of maximum absolute divergence (expansion / contraction center) or uses fixed bottom-center POV anchoring.
3. **Radial Projection**: Projects 2D flow vectors radially from the center with edge-weighting to cancel camera movement.
4. **Scene Cut Reset**: Resets numerical integration on detected cuts to prevent drift.
5. **Polynomial Detrending & Normalization**: Removes baseline drift with Hanning-windowed regression and scales to $[0, 100]$.
6. **Keyframe Reduction**: Detects inflection points (peaks and valleys) to produce clean, compact hardware-ready actions.

---

## License
- `open-fungen`: Dual-licensed under [Apache-2.0](open-fungen/LICENSE-APACHE) and [MIT](open-fungen/LICENSE-MIT).
- `FunscriptFlow`: Licensed under [Apache-2.0](LICENSE).
