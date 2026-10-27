# ⚡ Pulsar — Neural Kinematic Workstation & Funscript Generator

[![CI](https://github.com/gamerman420691337-arch/pulsar-fs/actions/workflows/ci.yml/badge.svg)](https://github.com/gamerman420691337-arch/pulsar-fs/actions/workflows/ci.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust: 1.80+](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20a%20Coffee-Donate-orange?style=for-the-badge&logo=buy-me-a-coffee)](https://buymeacoffee.com/thesmartestgooner)

Pulsar is a local motion workstation under active implementation. **Pulsar Desktop**, the command-line client, and the experimental local browser player use one authenticated Engine. The roadmap remains **PARTIAL**: useful model defaults, real-media accuracy, supported-platform releases and physical-device synchronization are not qualified.

Start with the [current app guide and model-free demo](docs/PULSAR_APP_QUICKSTART.md). The [execution dashboard](docs/PULSAR_MILESTONES.md), [generated current status](docs/PULSAR_CURRENT_STATUS.md), [software specification](docs/PULSAR_SOFTWARE_SPECIFICATION.md), and [architecture-first roadmap](docs/PULSAR_MAJOR_ROADMAP.md) separate implemented behavior from remaining requirements.

## Open the desktop app

Build both companion executables from this checkout:

```sh
cargo build -p pulsar -p pulsar-worker
```

Run the resulting `pulsar` executable with no arguments or with `gui`. Keep `pulsar-worker` beside it. On the development workstation, the current validation build is:

```sh
/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar gui
```

The model-free demo needs no model download. Media decoding and visual inference need the explicitly configured FFmpeg/FFprobe and ONNX runtime respectively. A source build is not a qualified portable release. Older installed shortcuts and retained runtime candidates can contain older software; use the executable belonging to the checked source.

## What you can use

| Surface | Current workflow |
| --- | --- |
| Studio / Cinema | Durable projects, captured media, source-time preview and editable motion timelines. |
| Generator | Explicit synthetic patterns, script import, audio-derived motion and visual generation with admitted model/profile settings. Generation produces candidates for separate review and commitment. |
| Doctor | Shared script diagnostics and explicit repairs. |
| Library batches / Stash | Bounded media discovery and retained generation/publication workflows. Installed-Stash compatibility remains unqualified. |
| Portable projects | Explicit private project packaging and clone import, plus neutral script exports. |
| Automation / Settings | Explicit local assistant configuration and proposals; useful default model selection remains open. |
| Devices | Handy inspection, device-motion proposals and a [guarded CLI playback path](docs/PULSAR_HANDY_PLAY_SUBMISSION.md). Desktop playback and physical qualification remain open. |

Existing authored work, protected regions, revisions and original request identities are preserved by the shared Engine contracts. Unsupported options refuse rather than silently selecting another implementation. Six-axis synthetic patterns and neutral exports are implemented; that does not establish faithful six-axis recovery or physical playback.

## Try an offline script demo

This command creates a retained project, generates and commits a deterministic synthetic Stroke pattern, and exports to a **new** destination:

```sh
pulsar synthesize 'pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8' --output /absolute/new-demo.funscript
```

Use the current executable's full path if `pulsar` on PATH is an older build. Expected output is 17 actions, at 125 ms intervals from 0 to 2000 ms, with positions between 40 and 60. This is authored motion, not video inference or natural-language AI. Existing destinations refuse; projects from unsuccessful attempts may remain available for inspection. The [app guide](docs/PULSAR_APP_QUICKSTART.md) provides the equivalent desktop workflow with separate review/commit steps.

## Local browser player

In the desktop, open **Browser live**, capture the Generator’s explicit file model/profile, use its [owner-selected pack](docs/PULSAR_LIVE_MODEL_PACKS.md), or choose a live-options JSON file, review the live settings, and click **Launch browser player**. The open project and revision are captured for the launch. Open its visible link; **Stop browser player** requests cleanup and joins the owned server. See [desktop launcher](docs/PULSAR_DESKTOP_BROWSER_LIVE.md) and [direct live configuration](docs/PULSAR_DESKTOP_LIVE_OPTIONS.md). The CLI remains available:

```sh
pulsar browser-live PROJECT REVISION /absolute/live-options.json
```

Open its printed loopback URL, select a local video, and press Play. An explicit compatible temporal model/profile and runtime are required. Recording, candidate review/commit, explicit gap proposals, and gap-free committed Stroke download are available. Experimental **Share a browser tab** and [**Use camera**](docs/PULSAR_BROWSER_CAMERA.md) provide explicit capture input paths with distinct presentation-time declarations. Actual tab/camera acquisition, source-player clock mapping and platform acceptance remain open. The player sends no toy commands. Verified diagnostic-model feedback is not useful trained motion.

See [launch instructions](docs/PULSAR_BROWSER_LIVE_PREVIEW.md), [tab-sharing limits](docs/PULSAR_BROWSER_TAB_CAPTURE.md), and [retained timing domains](docs/PULSAR_LIVE_TIME_BASIS.md).

## Media, library and Stash workflows

Use [declared video views](docs/PULSAR_VIDEO_VIEWS.md) for stereo/VR180/360 projections and [desktop library batches](docs/PULSAR_DESKTOP_LIBRARY_BATCHES.md) for retained local scheduling. [Stash integration](docs/PULSAR_STASH_INTEGRATION.md) documents the brokered endpoint, encrypted credential names, server-to-local mappings, opt-in scan/tag phases and recovery. Do not put key values in command lines or project files.

For actual command syntax, run `pulsar --help` and `pulsar COMMAND --help`. Historical model names, legacy CLI flags and compatibility parsing do not prove current model availability or supported behavior.

## Development checks

```sh
cargo test --workspace
node scripts/check-architecture.mjs
node scripts/architecture-source-guards.mjs
```

Run the relevant feature/backend checks for changes in those paths. Authored fixtures, headless controllers, browser mocks and compilation have narrower scope than rendered GUI, real-media, platform, device or release acceptance. The [current status](docs/PULSAR_CURRENT_STATUS.md) links source-bound receipts and outstanding gates.

## 📄 License

Pulsar is currently licensed under the **GNU Affero General Public License v3.0** ([LICENSE](LICENSE)) during pre-1.0 development.

> **Roadmap Note:** Pulsar will explicitly transition to the **Apache License, Version 2.0 (Apache-2.0)** upon the official 1.0 General Availability release.
