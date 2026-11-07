# Offline authored input workflows

2026-10-12. Fifteen scoped shipped-CLI checks PASS; full roadmap PARTIAL. This finite authored campaign is a component/workflow result, not Core/Complete or media/model/platform/device qualification.

The actual offline updater publishes a new candidate with the current App and Worker plus thirteen independently verified unchanged files from the retained runtime. It reports two provided files and thirteen reused files, with candidate manifest SHA-256 `8c29c01f96d67e22bb5126193b33b515fbd7de630c71454139259ea181592685`. The candidate contains media tools, CPU runtime and assistant assets, but no installed generation pack or useful-default qualification. No model or assistant inference is run here.

The permanent `scripts/qualify-offline-inputs.py` harness verifies every declared candidate file, starts its copied Engine, and uses its copied CLI and Worker with executable-relative media discovery. The campaign runs inside a network-isolated bubblewrap namespace, with no media/runtime tool overrides, private owned state and bounded operation/startup deadlines. It stops and joins both owned Engine lifetimes. It does not connect to the user's library, devices or an inference service.

| Family / case | Scoped evidence | Completion boundary |
| --- | --- | --- |
| Ordinary video | Authored MPEG-4, 64×64, eight frames/second; sixteen exported checked actions through Fast CLI generation. | No learned detector/target model. Baseline global-motion review remains; this does not prove GEN-01 faithful target recovery or numerical quality. |
| Stereo / VR180 / 360 | Authored MPEG-4 with explicit left/right side-by-side selection and declared flat/half-equirectangular/equirectangular viewports; each exports sixteen checked actions. | Workflow admission and execution only; no real VR correctness or differential eye/pixel qualification in this campaign. |
| Audio-only | Authored AAC envelope mapping exports checked synthesized stroke actions. | RMS synthesis, not musical beat or semantic interpretation. |
| Mixed audio/video | Copy-muxed authored source; faithful policy retains exactly the ordinary video's exported actions. | Per-fixture equality, not general multimodal accuracy. |
| Text / image + prompt | Strict triangle pattern exports 101 checked actions, with explicit image input for the image path. | Deterministic syntax; no free-form local-AI or semantic image conditioning claim. |
| Existing script | Import, explicit candidate commit and selected-axis export preserve all six authored actions and the original file hash; the exact project snapshot survives Engine restart. | One standard stroke script variant, not all interchange/edit/recovery qualification. |
| Refusals | Damaged leading AAC, damaged MPEG-4, unsupported custom legacy FPS, free-form deterministic-pattern input and missing image prompt all refuse without a destination script. | First two connect preceding decoder repairs to the shipped CLI/Engine/Worker. They do not cover arbitrary corruption or every failure cut. |
| Live / device | Not exercised. | IN-08 and live/device gates remain open. |

The first harness run incorrectly requested `--fps 8`. Source inspection confirms that non-default legacy FPS is deliberately unsupported at this entry point; that refusal is retained. The final harness uses the supported default settings and separately checks the unsupported option's refusal. No public option is silently treated as implemented.

Evidence: [checkpoint](evidence/roadmap-2026-10-12/offline-inputs/checkpoint.json), with command transcripts, checked output scripts, authored sources/views, update receipt and candidate membership. Fifteen checks pass through twenty-one CLI invocations. 431 architecture checks and 36 source guards pass. Rust sources and the shared App/Worker binaries are unchanged from the preceding corruption checkpoint; no new Cargo build, lint or library runtime suite is claimed. All evidence stays bound to this subject. Local session credentials and private Engine state are excluded from the packet and removed with owned scratch; borrowed assets and caches remain intact.

IN-09 requires an actual Core bundled Fast edition across every required family, including local interpretation and live behavior. This campaign does not close IN-09, any milestone, Default/Maximum delivery, independent useful-quality scoring, real-media compatibility, other platforms/vendors, physical Handy or human/release acceptance. No reference labels, script timing checks or acceptance records were invented.
