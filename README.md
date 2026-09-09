# ⚡ Pulsar — Funscript Generator (`pulsar`)

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20a%20Coffee-Donate-orange?style=for-the-badge&logo=buy-me-a-coffee)](https://buymeacoffee.com/thesmartestgooner)

> **The Neural Kinematic Workstation & Funscript Generator**  
> *Composed Monolith Architecture • Pure Native Rust Engine • Adaptive Fast/Slow ML • 6-DOF Robotic Kinematics • Zero External C Dependencies*

Pulsar provides an ultra-fast, zero-runtime-dependency desktop workstation and unified CLI accessible via the **`pulsar`** command (with backward-compatible aliases for `fs-hub` and `open-fungen`).

---

## Key Features

### 📑 1. Studio Editor & Multi-Axis 6-DOF Timeline
- **6-DOF Multi-Axis Support**: Full multi-track coordination for `Stroke` (L0), `Surge` (L1), `Sway` (L2), `Pitch` (R1), `Roll` (R2), `Twist` (R0), and `Suction` (V0).
- **Bounded Undo / Redo Engine**: 50-state circular snapshot buffer (`Ctrl+Z`, `Ctrl+Y`, `Ctrl+Shift+Z`, and transport ribbon buttons) for zero-risk editing.
- **Intuitive Navigation**: Click-to-seek playhead positioning, fluid track scrubbing without modifiers, and binary-searched viewport culling for 100k+ keyframe datasets at 144+ FPS.
- **Magnetic Audio Transient Snapping**: Neon-cyan guidelines and `⚡ SNAP` badges automatically lock keyframes to audio beats and onset transients (`M` toggle).
- **Ergonomic Keyframe Nudging**: ArrowUp / ArrowDown adjustments (+/- 1 unit, or +/- 5 with Shift).

### 🎬 2. Cinema Player & 3D Robotic Rig Simulator
- **Real-Time 3D Forward-Kinematics Rig Simulator**: Live 60+ FPS interactive 3D rendering of OSR2 and SR6 robotic hardware reacting to active 6-DOF telemetry.
- **Distraction-Free Playback**: Floating auto-hiding HUD with live haptic gauge, real-time T-Code feedback, and one-click switching (`F1` / `F2`).
- **VR Side-by-Side (SBS)**: Stereo 3D video playback with adjustable IPD convergence offsets.
- **Multi-Speed Transport**: Instant playback speed switching (`0.5x`, `1.0x`, `1.5x`, `2.0x`, `[` / `]`).

### 🩺 3. Script Doctor & 1-Click Auto-Repair
- **Hardware Certification Linter**: Real-time diagnostics detecting excessive physical velocities (>450 u/s), non-monotonic timing inversions, and high-frequency micro-jitters.
- **⚡ Fix All Issues**: 1-click safe atomic repair with viewport centering and undo protection.

### ⚙️ 4. Kinematics, S-Curve Physics & Thermal Protection
- **Jerk-Limited S-Curve Profiles**: Quintic smoothstep transitions preserving apex extrema while eliminating harsh mechanical reversals.
- **Thermal Wattage Dissipation Model**: Continuous motor heating and exponential decay simulation with dynamic amplitude throttling to prevent actuator burnout.

### 🧠 5. Phase 8 Adaptive Fast/Slow Vision-to-Motion Machine Learning
- **Adaptive Failure Router**: Calibrated uncertainty metrics dynamically route intervals across `FastTracking` ($O(1)$), `SpecialistRefresh` (YOLOv12 / RF-DETR), `DenseRepair` (TAPNext++ / CoWTracker), and `SemanticReacquisition` (SAM 3.1).
- **YOLO26 & RF-DETR Direct Decoders**: Ultra-fast NMS-free & DFL-free direct top-$K$ tensor decoding.
- **TAPNext++ Point Trajectories**: Sub-pixel multi-point sliding-window tracking with forward-backward cycle consistency.
- **Multi-Hypothesis Bayesian Reconciler**: Probabilistic hypothesis tree resolving occlusions with audio transient grounding.
- **5 Empirical Pareto Profiles**: `Economy`, `BalancedSpecialist`, `GenericDefault`, `DenseOffline`, and `GeometryHeavy3D`.

### 🔊 6. Sub-Millisecond Audio Waterfall Spectrogram & Multi-Band Infill
- **STFT Waterfall Spectrogram**: Real-time frequency power decomposition across Sub-Bass (20–120 Hz), Mid (120–2000 Hz), and High (2000–8000 Hz).
- **Automated Haptic Infill**: Synthesize frequency-locked haptic vibrations, rhythm-locked pulse infill, or companion channel modulation from audio tracks.

### 🔌 7. Hardware & VR Headset Synchronization
- **T-Code v0.2 / v0.3 Protocol Dispatcher**: Real-time Serial COM port (`/dev/ttyUSB0`) and UDP network streaming (`127.0.0.1:8888`) for OSR, Handy, Keon, and MultiFunPlayer.
- **VR Headset Telemetry Server**: Native embedded server receiving live timestamps and playback state from **DeoVR** and **HereSphere** VR video players.

### 🗄️ 8. Automation, Stash & Community Plugin SDK
- **Stash Media Server Integration**: Direct GraphQL synchronization querying scenes lacking scripts, batch-generating them, and triggering metadata re-scans.
- **Headless Batch Queue**: High-throughput multi-threaded worker processing entire video libraries in headless or background mode.
- **Procedural Macro SDK**: Pure mathematical signal transformers (e.g. S-Curve, Stochastic Micro-Jitter, Infill Generators).

---

## Keyboard & Mouse Shortcuts

| Shortcut | Action | Context |
| :--- | :--- | :--- |
| `Ctrl + Z` | Undo last editing change | Global / Studio |
| `Ctrl + Y` / `Ctrl+Shift+Z` | Redo last undone change | Global / Studio |
| `Ctrl + S` | Save funscript bundle | Global |
| `Ctrl + Shift + S` | Save funscript bundle as new file | Global |
| `Ctrl + O` | Open funscript file / bundle | Global |
| `Space` | Play / Pause playback | Global |
| `Delete` / `Backspace` | Delete currently selected keyframe | Studio Editor |
| `Up` / `Down` Arrow | Nudge selected keyframe position (+/- 1) | Studio Editor |
| `Shift` + `Up` / `Down` | Nudge selected keyframe position (+/- 5) | Studio Editor |
| `Left` / `Right` Arrow | Step time cursor (-33ms / +33ms) | Global |
| `Shift` + `Left` / `Right` | Step time cursor (-1000ms / +1000ms) | Global |
| `Home` / `End` | Jump to start (0ms) / end of script | Global |
| `M` | Toggle magnetic audio transient snapping | Studio Editor |
| `[` / `]` | Decrease / Increase playback speed (0.25x) | Global / Cinema |
| `1` – `7` | Switch 6-DOF channel (Stroke, Surge, Sway...) | Studio Editor |
| `F1` – `F6` | Switch View (Studio, Cinema, Generator, Doctor, Sync, Automation) | Global |
| `Double-Click` | Insert keyframe at mouse position | Timeline |
| `Left-Click` | Seek playhead to clicked timestamp | Timeline |
| `Left-Click Drag` | Scrub playhead smoothly across canvas | Timeline |
| `Right-Click` | Delete hovered keyframe | Timeline |
| `Middle-Click Drag` / `Ctrl-Drag` | Pan timeline viewport horizontally | Timeline |
| `Scroll Wheel` | Zoom timeline in / out | Timeline |

---

## Installation & Quick Start

### Quick Install (Linux / macOS)
Clone the repository and run the automated installer:
```bash
git clone git@github-gamerman:gamerman420691337-arch/Funscript-Hub.git
cd Funscript-Hub
./install.sh
```
This builds the release binary with maximum compiler optimizations and symlinks `pulsar` (and aliases `fs-hub`, `open-fungen`) into `~/.local/bin/`.

### Launch the Desktop GUI
```bash
pulsar
# or legacy alias:
fs-hub
```

---

## CLI Usage

### 1. Audit a Funscript with Script Doctor
```bash
pulsar doctor video.funscript --max-speed 450.0
```

### 2. Automatically Repair a Funscript
Clamps motor speed violations, removes micro-jitters, sorts, and deduplicates:
```bash
pulsar fix input.funscript -o output.fixed.funscript
```

### 3. Generate from Video (Adaptive Fast/Slow Neural Mode)
```bash
pulsar generate video.mp4 \
  --model ~/.cache/pulsar/models/FunGen-12n-pov-1.1.0.onnx \
  --profile balanced \
  --conf 0.35 \
  --fps 30.0 \
  --pov \
  --multi-axis
```

### 4. Generate from Video (Optical Flow Mode)
```bash
pulsar generate video.mp4 --fps 30.0 --pov --multi-axis
```

### 5. Multi-Band Audio Spectral Synthesizer & Infill
```bash
pulsar audio-synth video.mp4 --mode infill --band sub-bass
```

### 6. Headless Batch Processing
Process an entire folder of videos in headless queue mode:
```bash
pulsar batch /path/to/videos --multi-axis --fps 30.0
```

### 7. Stash Media Server Integration
Query scenes missing funscripts and batch-generate directly into Stash directories:
```bash
pulsar stash query --limit 25
pulsar stash auto-generate --limit 10 --multi-axis
```

---

## Running Tests

Run the full Rust test suite across all 11 modules:
```bash
cargo test
```

Run Python legacy test suite:
```bash
pytest test_funscript_flow.py
```

---

## License
- `open-fungen` / `fs-hub`: Dual-licensed under [Apache-2.0](open-fungen/LICENSE-APACHE) and [MIT](open-fungen/LICENSE-MIT).
- `FunscriptFlow`: Licensed under [Apache-2.0](LICENSE).
