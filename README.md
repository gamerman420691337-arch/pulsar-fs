# Funscript Hub (`fs-hub`)

> **The Unified, Open-Source, SOTA Funscript Generation, Precision Editing, and Hardware Synchronization Suite**  
> *Composed Monolith Architecture • Pure Native Rust Engine • Zero External C Dependencies*

`Funscript-Hub` provides an ultra-fast, zero-runtime-dependency desktop application and unified CLI accessible via the **`fs-hub`** command.

---

## Key Features

### 📑 1. Studio Editor & Multi-Axis 6-DOF Timeline
- **6-DOF Multi-Axis Support**: Full multi-track coordination for `Stroke` (L0), `Surge` (L1), `Sway` (L2), `Pitch` (R1), `Roll` (R2), `Twist` (R0), and `Suction` (V0).
- **Bounded Undo / Redo Engine**: 50-state circular snapshot buffer (`Ctrl+Z`, `Ctrl+Y`, `Ctrl+Shift+Z`, and transport ribbon buttons) for zero-risk editing.
- **Intuitive Navigation**: Click-to-seek playhead positioning, fluid track scrubbing without modifiers, and binary-searched viewport culling for 100k+ keyframe datasets at 144+ FPS.
- **Magnetic Audio Transient Snapping**: Neon-cyan guidelines and `⚡ SNAP` badges automatically lock keyframes to audio beats and onset transients (`M` toggle).
- **Ergonomic Keyframe Nudging**: ArrowUp / ArrowDown adjustments (+/- 1 unit, or +/- 5 with Shift).

### 🎬 2. Cinema Player Mode
- **Distraction-Free Playback**: Floating auto-hiding HUD with live haptic gauge, real-time T-Code feedback, and one-click switching (`F1` / `F2`).
- **VR Side-by-Side (SBS)**: Stereo 3D video playback with adjustable IPD convergence offsets.
- **Multi-Speed Transport**: Instant playback speed switching (`0.5x`, `1.0x`, `1.5x`, `2.0x`, `[` / `]`).

### 🩺 3. Script Doctor & 1-Click Auto-Repair
- **Hardware Certification Linter**: Real-time diagnostics detecting excessive physical velocities (>450 u/s), non-monotonic timing inversions, and high-frequency micro-jitters.
- **⚡ Fix All Issues**: 1-click safe atomic repair with viewport centering and undo protection.

### ⚙️ 4. Kinematics, S-Curve Physics & Thermal Protection
- **Jerk-Limited S-Curve Profiles**: Quintic smoothstep transitions preserving apex extrema while eliminating harsh mechanical reversals.
- **Thermal Wattage Dissipation Model**: Continuous motor heating and exponential decay simulation with dynamic amplitude throttling to prevent actuator burnout.

### 🧠 5. SOTA AI Neural & Optical Flow Generation
- **Neural YOLOv12 / SAM 2 Tracking**: Anatomical detection (`penis`, `glans`, `pussy`, `butt`, etc.) with occlusion recovery and Non-Maximum Suppression via ONNX Runtime (`ort`).
- **Rayon Data-Parallel Optical Flow**: Contiguous gradient buffers yielding ~4x Lucas-Kanade speedups with automated photometric cut detection.
- **Automated 6-DOF Derivation**: Auto-generates companion `.surge.funscript`, `.sway.funscript`, and `.pitch.funscript` files from camera and motion geometry.

### 🔌 6. Hardware & VR Headset Synchronization
- **T-Code v0.2 / v0.3 Protocol Dispatcher**: Real-time UDP network streaming (`127.0.0.1:8888`) for OSR, Handy, Keon, and MultiFunPlayer.
- **VR Headset Telemetry Server**: Native embedded server receiving live timestamps and playback state from **DeoVR** and **HereSphere** VR video players.

### 🗄️ 7. Automation, Stash & Community Plugin SDK
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
This builds the release binary with maximum compiler optimizations and symlinks `fs-hub` into `~/.local/bin/fs-hub`.

### Launch the Desktop GUI
```bash
fs-hub
```

---

## CLI Usage

### 1. Audit a Funscript with Script Doctor
```bash
fs-hub doctor video.funscript --max-speed 450.0
```

### 2. Automatically Repair a Funscript
Clamps motor speed violations, removes micro-jitters, sorts, and deduplicates:
```bash
fs-hub fix input.funscript -o output.fixed.funscript
```

### 3. Generate from Video (AI Neural Mode)
```bash
fs-hub generate video.mp4 \
  --model models/FunGen-12n-pov-1.1.0.onnx \
  --conf 0.35 \
  --fps 30.0 \
  --pov \
  --multi-axis
```

### 4. Generate from Video (Optical Flow Mode)
```bash
fs-hub generate video.mp4 --fps 30.0 --pov --multi-axis
```

### 5. Headless Batch Processing
Process an entire folder of videos in headless queue mode:
```bash
fs-hub batch /path/to/videos --multi-axis --fps 30.0
```

### 6. Stash Media Server Integration
Query scenes missing funscripts and batch-generate directly into Stash directories:
```bash
fs-hub stash query --limit 25
fs-hub stash auto-generate --limit 10 --multi-axis
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
