> Historical snapshot preserved during the 2026-10-27 dashboard reconciliation. Current execution state is maintained in [the execution dashboard](PULSAR_MILESTONES.md). Historical “current” labels apply only to their recorded checkpoints. Requirements and retained evidence are not waived.

# Pulsar milestone gate ledger

Date: 2026-10-11. Full roadmap: INCOMPLETE. This is the detailed milestone artifact named by [the major roadmap](PULSAR_MAJOR_ROADMAP.md). The [software specification](PULSAR_SOFTWARE_SPECIFICATION.md), [architecture decisions](PULSAR_ARCHITECTURE_DECISIONS.md) and [architecture-first execution plan](PULSAR_ARCHITECTURE_FIRST_EXECUTION_PLAN.md) remain authoritative. Gate identifiers below organize work; they do not replace specification requirements or acceptance thresholds.

No milestone is closed by this ledger. A historical receipt supports its recorded source/binary/model/environment subject, not every later checkout. PASS below refers only to completed named checks; missing qualification is UNKNOWN or INCOMPLETE, not presumed successful. The current regression checkpoint is [Engine and client regression](evidence/roadmap-2026-10-11/engine-regression/checkpoint.json).

## M1: baseline and stroke correctness foundation

| Gate | Required result and evidence | Current boundary / next proof |
| --- | --- | --- |
| M1-GEOMETRY | Model-specific decode, coordinates, crop/projection transforms, real preview/generation consumers and stable target identity. Trace exact tensors, pixels, PTS, model and source identities. | Authored geometry/worker contracts have regression coverage. Revalidate real source/model/view consumers; a green helper test does not prove faithful tracking. |
| M1-MOTION | Preserve pauses/shallow excursions, prevent invented flat-signal strokes and amplitude expansion, retain timestamp/identity/reacquisition boundaries. | Deterministic invariants are separate from real-media correctness. Compare independently checked references and retain missing/extra/reversal/travel errors. |
| M1-DATA | Source-matched target/view annotations, checked script alignment, curated category coverage and independent held-out scoring. | User reports no existing annotations. Live references are permitted for training; sterile references are evaluation-only. Do not fabricate labels or promote optimizer fitting into useful quality. |

Evidence entry points: [training pixel correspondence](PULSAR_TRAINING_PIXEL_CORRESPONDENCE.md), [annotated training views](PULSAR_ANNOTATED_TRAINING_VIEWS.md), [reference capacity](PULSAR_REFERENCE_CAPACITY.md). Inspect their exact subjects before inheriting claims.

## M2: shared generation and editable projects

| Gate | Required result and evidence | Current boundary / next proof |
| --- | --- | --- |
| M2-DOMAIN | Shared GUI/CLI/batch/live motion semantics, neutral six-axis master, timestamp/clock provenance, source/candidate identity and explicit uncertainty. | Validate the same operation through actual consumers; detached staging or helper output is insufficient. |
| M2-EDITS | Protected edits, explicit unlock/replacement, candidate review/commit, rebase, undo/redo and restart preservation. | Authored edit/recovery contracts and selected native desktop scenarios exist. Broader crash, storage, rendered workflow and human effort remain independent. |
| M2-INTERCHANGE | Revision-bound native project migration, per-axis result/partial status, standard funscript export, archival exact receipt recovery and Stash publication without replacing existing work. | Preserve old models/pipeline provenance and source-time semantics; damaged/unknown schemas must refuse explicit recovery. |

Evidence entry points: [assistant edit recipes](PULSAR_ASSISTANT_EDIT_RECIPES.md), [saved edit recovery](PULSAR_ASSISTANT_EDIT_RECOVERY.md), [desktop edge trim](PULSAR_DESKTOP_EDGE_TRIM.md), [annotated Stash publication](PULSAR_ANNOTATED_STASH_PUBLICATION.md).

## M3: adaptive quality tiers and local assistance

| Gate | Required result and evidence | Current boundary / next proof |
| --- | --- | --- |
| M3-MODELS | Real pinned Fast/Default/Maximum pipelines, model-specific adapters and actual active provider disclosure; no placeholder substitution. | Distinct bounded visual-detector Fast/Default/Maximum plans execute with a pinned CPU model and candidate-bound receipts. This does not select useful default models, establish quality/speed floors, cover other input families or qualify GPU vendors. Rebenchmark permitted candidates before targeted fitting. |
| M3-ASSISTANT | Local structured target/settings/review/edit proposals, current owner/project/revision binding, separate approval and protected undoable effects. | Actual pinned Qwen3 proposal/status gates and authored effect/recovery tests are scoped evidence. Bundled/default assistant selection and useful task quality remain open. |
| M3-MEMORY | MEM-01/MEM-02 and TEST-13: real generation pause/checkpoint, assistant response, resumed job and protected live playback under contention. | Logical admission loans, exact-handle retirement and automatic release-driven retry have tests. A separate pinned YOLO11/Qwen3 CPU run pauses and automatically completes the same job from its unchanged initial input basis. First-attempt inference before pause, partial progress, GPU/native/live continuity and whole-application peaks remain open. |

Evidence entry points: [local assistant worker](PULSAR_LOCAL_ASSISTANT_WORKER.md), [journal admission](PULSAR_ASSISTANT_JOURNAL_ADMISSION.md), [memory handoff](PULSAR_ASSISTANT_MEMORY_HANDOFF.md), [capacity recovery](PULSAR_ASSISTANT_CAPACITY_RECOVERY.md), [native CPU contention](PULSAR_NATIVE_ASSISTANT_HANDOFF.md), [visual tracking plans](PULSAR_VISUAL_TRACKING_TIERS.md), [desktop/Stash tier admission](PULSAR_TIER_ADMISSION.md).

## M4: multimodal and six-axis generation

| Gate | Required result and evidence | Current boundary / next proof |
| --- | --- | --- |
| M4-INPUTS | All specified file/stream/audio/text/still-image-plus-prompt/script input families; clear unsupported/undecodable/partial results. | Qualify a finite container/codec/scheme/script matrix and prompt handling through shipped consumers. Core baseline coverage must work without optional packs. |
| M4-PRECEDENCE | Faithful visual motion takes precedence over conflicting rhythm; creative reinterpretation requires explicit intent. | Keep faithful scoring separate from the agreed creative rubric, timing/pattern constraints and correction effort. |
| M4-AXES | Neutral six-axis outputs, VR/stereo/projection handling, explicit per-channel lineage, observability and limitations. | Secondary-axis quality and creative metrics require their own criteria and independent evaluation. Missing axes cannot silently truncate prior scripts. |

Evidence entry points: [annotated portable generation](PULSAR_ANNOTATED_PORTABLE_GENERATION.md), [annotated Stash batches](PULSAR_ANNOTATED_STASH_BATCHES.md), [reference clock declarations](PULSAR_REFERENCE_CLOCK_DECLARATIONS.md). Authored fixtures do not qualify every supported modality/category.

## M5: live scheduling and devices

| Gate | Required result and evidence | Current boundary / next proof |
| --- | --- | --- |
| M5-LIVE | GPU fastest-preset live six-axis; CPU best-effort bounded latest-frame queues, skipped analysis and visible actual delay without stale backlog. | Measure capture-to-command, presentation synchronization, physical lag, sustained throughput and jitter separately. The 200 ms target's statistical definition remains open; buffered mode does not repair overload. |
| M5-DEVICE | Handy/Handy 2 online and offline BLE, upload consent, arming/disarming, stop/expiry/authority-loss behavior and stale command handling. | Handy 2 Pro is selected; firmware and encrypted credential setup are unavailable. Codec/RPC fixtures do not prove physical response, stop or synchronization. |
| M5-ADAPTATION | MOT-02 through MOT-05: neutral master preserved, timing preserved, amplitude/dynamics/coupled-axis limits enforced and inability disclosed. | Use qualified device-specific travel/velocity/acceleration/jerk limits; do not assume universal values or normalize shallow strokes to full travel. |

Evidence entry points: [live CUDA release measurement](PULSAR_LIVE_CUDA_RELEASE_MEASUREMENT.md) and the specification's device/live test scenarios. Native captured frames and software stop receipts remain distinct from physical-device acceptance.

## M6: portable editions, vendors and platforms

| Gate | Required result and evidence | Current boundary / next proof |
| --- | --- | --- |
| M6-EDITIONS | Core includes all baseline Fast input families, local assistant, editing/export and offline playback. Complete is the default full-offline edition and includes every advertised installed tier. | First launch must not require a network download or user-installed Cargo/Python/inference service. Deliver and execute actual contents, not a manifest claiming missing assets. |
| M6-INSTALL | Transactional integrity-checked extraction/import/update, interruption/damage/incompatibility recovery, portable data roots and relocation. | Existing pack/lifecycle tests are scoped evidence. Qualify actual Core/Complete artifacts, disk/extraction limits, interrupted update/rollback, portable credentials and relocation. |
| M6-MATRIX | Windows single executable, Linux AppImage, Apple Silicon macOS support and proposed app/dmg packaging; genuine qualified NVIDIA/AMD/Apple/Intel acceleration for every tier. | Pin exact OS/architecture/driver/backend/model/device subjects. CPU fallback must be disclosed. Other architecture priorities and signing/notarization policy remain as specified. |
| M6-MEMORY | 16 GB system floor; qualified per-tier RAM/VRAM, startup and response deadlines; avoid unified-memory double counting. | The suggested 12 GB application budget is not an approved hard cap. NVIDIA reference performance/memory configuration cannot silently substitute another board or generalize to other vendors. |

Relevant specification families: PKG, HW, MEM and platform architecture priorities. [Annotated model packs](PULSAR_ANNOTATED_MODEL_PACKS.md) and [model-pack withdrawal](PULSAR_MODEL_PACK_WITHDRAWAL.md) are component evidence, not edition-delivery qualification.

## M7: Public Preview

| Gate | Required result and evidence | Current boundary / next proof |
| --- | --- | --- |
| M7-QUALITY | Per-category Default stroke precision and recall at least 80%, target 90%; material-error flag recall at least 95% against independent annotations. | Labels, category protocol and checked source/reference alignment are missing for qualification. Retain >100 ms reversal deviation and >20% neutral travel error as material errors. |
| M7-SPEED | Required Fast stroke-only paths >1x realtime; reference GPU Fast at least 10x. CPU Default at least 0.5x, reference GPU Default at least 5x. | Measure complete decode/inference/process/export on the specified 30-minute 1080p/30-fps workload and exact hardware. Do not convert worker-only timing into end-to-end speed. |
| M7-PREVIEW | Measured human correction effort, applicable test matrix, explicit experimental labels for unqualified six-axis playback and actual preview artifacts. | The five-human-hour annotation pilot is a method trial, not release coverage. Required regression, native, storage/platform, media/device and human/release gates remain independent. |

## M8: Stable and comparative qualification

| Gate | Required result and evidence | Current boundary / next proof |
| --- | --- | --- |
| M8-QUALITY | Per-category Default precision and recall at least 95%, qualified material-error detection, creative/secondary-axis criteria and physical six-axis behavior. | No Stable acceptance is established by optimizer fitting, authored fixtures or compiled backends. |
| M8-EFFORT | For at least 95% of 30-minute clips per category, no more than five minutes editing and ten minutes total human effort. | Measure a declared independent human protocol. The 95th-percentile effort agreement does not define the live latency percentile. |
| M8-COMPARISON | Reproducible exact-source/provider/model/hardware comparisons and SOTA claims only after complete supporting evidence. | Freeze splits, scoring, preprocessing, artifacts and candidate selection; never use sterile evaluation inputs for training or invent signatures/human acceptance. |

## Cross-milestone qualification obligations

All explicit TEST scenarios, security/consent requirements, delivery/platform/vendor combinations, open-policy decisions and assurance obligations in the specification/execution plan remain required. A selected test subset cannot close the whole test matrix. Before closing a gate, retain exact subject identities, command/operation results, refusals and interruption outcomes, applicable rendered/native/media/device observations, unresolved failures and the required independent human/release authority. Missing evidence stays open.

Historical scoped Stash evidence: [real Stash compatibility](PULSAR_REAL_STASH_COMPATIBILITY.md) exercised official Stash v0.31.1 with a fresh owned library and authored media on its recorded 2026-10-09 subject. It does not establish current user-library compatibility, every later source version, remote script hash verification or model/media qualification.


M6 relocation component update: [portable approved assistant policy](PULSAR_PORTABLE_ASSISTANT_POLICY.md) supports moving policy and runtime/model assets together without changing identity declarations. This scoped Linux host-policy result does not deliver Core/Complete artifacts or qualify the platform matrix.


M6 packaging component update: [retained static assistant candidate](PULSAR_STATIC_ASSISTANT_BUNDLE.md) supplies a pinned assistant runtime/model with the dedicated worker and media tools, and supports offline host approval after relocation. Actual twelve-file assembly succeeds on ordinary storage. Generation model packs, edition defaults, installation/upgrade and the platform matrix remain open; current-host ELF requirements and transitive licensing are recorded explicitly. No milestone is closed.


M6 runtime delivery update: [bundled CPU runtime](PULSAR_BUNDLED_CPU_RUNTIME.md) resolves and pins executable-relative ONNX Runtime without a runtime override. Actual copied relocated CLI/Engine/worker generation passes for authored direct-profile and explicitly selected offline-pack paths. The retained combined candidate includes assistant and CPU runtimes; generation model defaults, actual editions, standard-system/confinement/OS/vendor/license coverage and other gates remain open.


M6 installation component update: [first-party offline runtime installation](PULSAR_OFFLINE_RUNTIME_INSTALLATION.md) verifies declared candidate bytes and publishes only a complete private stage into a new destination. It does not overwrite working installations or activate models. Recognized inactive stages now have [explicit inspection/discard](PULSAR_RUNTIME_STAGE_RECOVERY.md), with live-lock, stale-identity and unknown-content refusal. Legacy unmarked/corrupt stages require manual inspection. Single-file candidate extraction is now implemented as the scoped component below. Updates/rollback, signed actual Core/Complete delivery and full filesystem/platform/vendor qualification remain open.


M6 archive component update: [single-file offline candidate delivery](PULSAR_OFFLINE_RUNTIME_ARCHIVE.md) adds first-party packaging and bounded exact-byte extraction through the existing installation custody path. It preserves original manifest/payload identities and refuses corruption/overwrite. Actual editions/generation defaults, first-launch embedding, signatures, archive-temp recovery, platform/filesystem/vendor/license qualification and other model/media/device/human gates remain open. No milestone is closed.


M6 first-use/AppImage custody update: [persistent runtime launch](PULSAR_APPIMAGE_RUNTIME_CUSTODY.md) supplies actual AppImage assembly and integrity-checked first-use installation/cache reuse before Engine startup. It addresses the Engine's intentional lifetime beyond a transient client. The exact artifact/native outcomes are in its checkpoint. Actual edition model assets/defaults, broad FUSE/platform/vendor/license/signing and filesystem/update/rollback/media/device/human qualification remain open; no milestone is closed.


M6 cleanup component update: [downstream cleanup correction](PULSAR_APPIMAGE_CLEANUP.md) replaces the upstream zero-descriptor nftw call, retains exact modified-runtime source/build identity and restores the automatic cleanup requirement in the actual AppImage campaign. Its checkpoint distinguishes preceding failures, source regression and current artifact results. Interrupted/concurrent extraction, orphan handling, broad platform/signing/license/model/edition/media/device and human release gates remain open; no milestone is closed.


Graphical first-use software update: [setup presentation](PULSAR_RUNTIME_BOOTSTRAP_GUI.md) adds progress, bounded errors, retry and close before graphical runtime installation. Headless commands preserve their path. Native rendered workflow and human acceptance remain open; no milestone is closed.


Retained-runtime selection update: [explicit verified selection](PULSAR_RETAINED_RUNTIME_SELECTION.md) permits inspection and launch without the original supplier. This returns to selected assets while preserving damaged updates and existing Engine custody; it does not roll back databases or close signed update, compatibility, retention, storage/platform, model/device or human gates.


Model-delivery readiness inspection: [bounded current-source findings](PULSAR_MODEL_DELIVERY_READINESS.md) confirms the current candidate has no generation-model payloads/default path. A real local detector has broadly matching tensor dimensions, but observed supplier terms exclude repackaging/outside use. No weights were executed, copied, trained or activated. Generation bundling, eligible model selection, all baseline inputs and edition/model qualification remain incomplete.


Official-model integration update: [YOLO11 candidate probe](PULSAR_YOLO11_CANDIDATE_PROBE.md) retains pinned publisher weights, a checked FP32 export and actual confined CPU generation/preview evidence. Authored frames produce no detections, and standard export refuses unresolved motion evidence. No annotations exist; software work continues. Numerical/target quality, bundled defaults and release qualification remain open.


Bundled generation transport update: [inert declared pack delivery](PULSAR_BUNDLED_GENERATION_PACKS.md) passes actual real-model archive/cache/worker and AppImage campaigns, including nested tamper refusal and automatic cleanup. Engine787/zero failures/59ignored and both strict lint configurations pass. Discovery/admission, useful defaults, baseline families and edition/platform/model/device/human qualification remain open.


Bundled discovery update: [verified installed and retained runtime inventory](PULSAR_BUNDLED_MODEL_DISCOVERY.md) exposes pack identities and import paths without Engine startup, selection or native approval. Real-pack discovery/import/worker and actual AppImage campaigns pass, with Engine787/client230 and both strict lint configurations. Graphical admission, useful defaults and full model/edition/platform/device/human gates remain open.


Desktop bundled model update: [explicit import preparation](PULSAR_BUNDLED_DESKTOP_PREPARATION.md) displays asynchronously verified inventory in the Generator and fills the existing inert import controls without importing or approving execution. Client234, strict lint and actual package/process checks pass. Headless drawn-control tests do not establish native display or human acceptance; useful defaults and full model/edition/platform/device gates remain open.


Detector numerical update: [pinned same-input comparison](PULSAR_DETECTOR_NUMERICAL_COMPARISON.md) verifies bit-identical preprocessing and five detections against Torch and publisher NMS on one ordinary non-square image. This does not establish letterbox/default-predictor parity, target quality, useful presets, script timing, performance or release/device/human qualification; full roadmap status remains PARTIAL.


Detector preprocessing update: [explicit fixed letterbox](PULSAR_DETECTOR_LETTERBOX.md) adds a first-party profile option and shared admitted preview/generation adapter with exact source-view box inversion. Thirteen independent pixel fixtures and real-model numerical comparisons pass; default packs, useful target/script quality and full model/edition/platform/device/human gates remain open.


Detector score update: [checked generation setting](PULSAR_DETECTOR_SCORE_SETTINGS.md) replaces the misleading CLI default and carries optional finite scores through ordinary/desktop/Stash generation, with immutable configuration and effective worker lineage. Omitted requests and preview retain 0.25. Tests and the pinned public-image process campaign are recorded separately; target quality, useful presets and full model/edition/platform/device/human gates remain open. Full roadmap status remains PARTIAL.


Generation resolution update: [model-aware analysis dimensions](PULSAR_ANALYSIS_DIMENSIONS.md) uses declared detector dimensions for new CLI/desktop/Stash requests, preserves explicit overrides and fixed temporal/no-model defaults, and leaves persisted settings unchanged. Actual worker decode and source-clock evidence is recorded separately. Useful presets, real-media quality/performance, full edition/platform/device and human gates remain open; roadmap status remains PARTIAL.


Camera diagnostic update: [actual target/background flow evidence](PULSAR_CAMERA_MOTION_EVIDENCE.md) retains checked image-space translations and point counts in generation receipts. Six actual CPU detector/worker pairs from a known whole-image shift pass at model-sized and explicit small analysis sizes, with stable identities and constant exports. This controlled public-image fixture does not qualify camera/media coverage, target quality, physical motion or devices; full roadmap remains PARTIAL.


Multiscale flow update: [bounded larger image-motion support](PULSAR_MULTISCALE_FLOW.md) preserves accepted local evidence, checks coarse proposals at full resolution, and records search bounds and target sample counts. Controlled 6- and 18-source-pixel translations pass all six native pairs each; 36 pixels remains PARTIAL with one inconsistent pair and standard export refusal. Full library and perception suites, all-feature binaries, strict linting and architecture guards pass within their stated scope. No annotation, target-model, physical-device, human or release qualification is inferred; the full roadmap remains PARTIAL.


Relative-motion update: [independent target/background image-motion evidence](PULSAR_RELATIVE_MOTION_EVIDENCE.md) exposes and repairs a global association guard that blocked an independently associated selected target when unrelated detections were ambiguous. A per-update affected-identity set also blocks fresh ambiguous identities. The final actual native chain passes 12 known patch-relative pairs in two directions and both decode sizes, including exact predicted exports; 36-source-pixel camera motion remains PARTIAL. Default library/perception suites and all-feature binaries pass; qualification, real target annotations/models, platform, devices and human acceptance remain separate. Full roadmap remains PARTIAL.


Saved-view training intake update: [checked desktop-sidecar assembly](PULSAR_SAVED_VIEW_TRAINING_INTAKE.md) connects existing saved geometry to exact training manifest entries, retaining source/tool/plan bindings and private atomic no-replace publication. Five new intake tests and 20 existing staging/pixel/label regressions pass, including actual native planning/decode, independently expected authored labels, source/sidecar changes and output-preservation refusals. Human target/script alignment stays unverified; no real annotation, fitting or milestone closure is claimed. Full roadmap remains PARTIAL.


Component-runtime update: [offline changed-asset delivery](PULSAR_RUNTIME_COMPONENT_UPDATES.md) publishes a complete independent candidate from two supplied application/worker binaries and 13 verified retained assets, pinned to the base manifest. The actual offline CLI verifies all 15 payloads, preserves the base, launches both retained and updated versions, and refuses wrong pins, corrupt supplied files, missing changed files and existing destinations. All 1,199 default library tests pass (65 ignored), all-feature binaries and both strict Clippy configurations pass, and actual installer/archive interruption, corruption and no-overwrite regressions pass. Signed update trust, delta archives, storage retention, schema rollback and edition/model/platform/device/human qualification remain open. Full roadmap remains PARTIAL.


Changed-payload runtime transport: [single-file pinned update archives](PULSAR_RUNTIME_UPDATE_ARCHIVE.md) add bounded versioned offline delivery of an exact desired manifest and changed assets. The producer checks every source asset, including omitted unchanged files; extraction requires the retained base and independently verifies its reused bytes. Complete candidates retain independent copies and exact manifest bytes. No signed update trust, active-process switch, schema rollback or retention policy is inferred. Software/native evidence is recorded separately; full edition/model/media/platform/device/human qualification remains open and the roadmap remains PARTIAL.


Archive construction recovery: [explicit recognized-stage inspection/discard](PULSAR_RUNTIME_ARCHIVE_STAGE_RECOVERY.md) adds a locked durable receipt to full and changed-payload packers, protects live writers, binds discard approval to receipt and partial-file identity/change metadata, and preserves unknown contents or stale approvals. New local commands remove only recognized inactive stage entries and synchronize cleanup. Legacy opaque/incomplete stages, automatic retention/scavenging, broad filesystem/power-loss, signed editions and model/media/platform/device/human qualification remain open; full roadmap remains PARTIAL.


Axis-pattern client update: [explicit synthetic candidate workflow](PULSAR_AXIS_PATTERN_CANDIDATES.md) exposes the existing typed one-through-six-axis family in CLI and desktop using a shared bounded builder. Core family preflight is reused at protocol admission for duration and aggregate allocation checks. The actual CLI/SessionClient/Engine/worker exercise independently checks 102 actions, six selected-axis exports, unchanged master before explicit commit, invalid-input/no-job and revision refusal, controller agreement, undo/redo and restart. Rendered desktop/human evidence, faithful visual six-axis recovery, real tier models and the full media/device/platform/release gates remain open; full roadmap remains PARTIAL.


Creative-worker proposal update: [bounded learned pattern proposals](PULSAR_CREATIVE_PATTERN_PROPOSALS.md) add explicit host constraints and independent Core admission to the actual local worker, while ordinary Engine context continues to offer no creative action. The final learned triangle passes; hold and unavailable-yaw intent checks FAIL despite well-formed bounded outputs. Invalid host scope refuses before inference. Creative model selection, freshness-bound application approval, shared CLI/desktop wiring and broader rubric evidence remain open; no milestone closes and the full roadmap remains PARTIAL.


Creative grammar ordering repair: [pattern choices before parameters](PULSAR_CREATIVE_PATTERN_PROPOSALS.md) preserves schema values while emitting host-required properties first. A controlled comparison and rebuilt confined worker now pass both triangle and hold; unavailable yaw still fails by substituting sine stroke. Ordinary native schemas remain byte-identical, independent Core validation remains intact, and no application approval/effect was added. Engine regression, build/lint and structural gates are separate from the failed model-quality gate; the full roadmap remains PARTIAL.

Creative candidate approval update: [Engine and CLI synthetic candidates](PULSAR_CREATIVE_PATTERN_PROPOSALS.md) add explicit host scope, separate inference/generation approval and private recoverable declarations. Actual learned Linux Engine/CLI evidence matches 17 independent triangle samples, exact export, explicit commit and Undo/Redo, retry identity and stale refusal. Master motion remains unchanged until commit. Broader model intent quality, desktop controls and media/device/platform/human gates remain open; full roadmap PARTIAL.

Desktop creative review update: [synthetic proposal preparation and recovery](PULSAR_CREATIVE_DESKTOP.md) adds explicit channel/timing/amplitude controls, exact saved scope and pattern review, separate inference/generation approvals and original-ID retries. The actual desktop controller against an owned learned Engine yields independently checked triangle samples, export and Undo/Redo. Headless control construction is distinct from unavailable rendered GUI evidence. Broad model quality, offline defaults, media/platform/device/human gates remain open; full roadmap PARTIAL.

Creative pulse/channel repair: [Core-valid pulse duties and tuple channels](PULSAR_CREATIVE_PULSE_TUPLES.md) removes the native grammar's forced 50% duty and fixes one slot per declared host channel. Paired pinned learned probes honor both 25%/75% pulse requests; all-six-channel hold/triangle remain passing. Actual Engine/CLI pulse candidates match independently authored samples, exports and Undo/Redo. Unavailable-yaw intent still fails; broader creative/default-model, rendered GUI, media/platform/device/human gates remain open. Full roadmap PARTIAL.


Learned CUDA execution update: [native visual tier evidence](PULSAR_LEARNED_CUDA_TIERS.md) verifies the pinned YOLO11 Fast plan against original CUDA artifacts and candidate lineage on the owner's RTX 4090. The missing convolution cuDNN components are explicitly pinned in the test policy; All three rebuilt scoped native checks pass, each with four learned inferences, original CUDA artifacts, candidate binding, unchanged master and terminal retirement. This is not model-quality, default-model, speed/resource, vendor, media, physical or human/release qualification. Full roadmap PARTIAL; goal active.


Profile RAM admission update: [owner-bounded model claims](PULSAR_PROFILE_RAM_ADMISSION.md) replaces the fixed generation/preview RAM claim with the declared minimum above the existing floor, checks current owner capacity, and reserves the same bytes. Resumed budgets stay exact; CPU/output/time limits and qualification authority remain independent. Larger declared learned-model CPU requests pass; native CUDA and completed gates are separately recorded. Full roadmap PARTIAL; goal active.


Aggregate CPU admission update: [owner-bounded worker quotas](PULSAR_CPU_ADMISSION.md) adds CPU claims to the shared RAM/job/GPU pool and checks cgroup/assistant-loan consistency. Generation, preview, live, native capture, WASM and assistant worker paths use their declared quotas, retained through existing custody retirement. Impossible owner claims refuse before pausing generation. Completed checks and scoped native CPU-only contention are recorded separately; model/media/platform/device/human qualification remains open. Full roadmap PARTIAL; goal active.


Bundled media fixture update: [orientation and source-clock coverage](PULSAR_BUNDLED_MEDIA_FIXTURES.md) removes the regression's optional libx264 encoding dependency while retaining separate authored H.264 decoder coverage. MPEG-4 and H.264 cases require actual rotation metadata, unchanged decoded pixels, exact frame counts and independently expected source times. Completed bundled/system tool checks are recorded separately; full media/model/platform/device/human qualification remains open. Full roadmap PARTIAL; goal active.


## Semantic image runtime candidate — 2026-10-11

[Native pixel controls](PULSAR_SEMANTIC_IMAGE_RUNTIME.md) pass two authored circle/square cases through a static, confined CPU vision runtime with whole-body schema JSON. Current image generation remains prompt-conditioned; production vision/profile/custody/approval integration and task quality remain open. No training, real-media, GUI, device or release qualification is claimed. Roadmap: PARTIAL.


## Bound semantic image worker — 2026-10-11

[Image assistant worker foundation](PULSAR_SEMANTIC_IMAGE_WORKER.md) binds exact RGB/projector identities through capture, aggregate admission, confinement, strict response checks and retirement. Final default/all-feature Engine suites pass 835/849 tests, protocol 126, and five actual native parent tests including seven new scripted vision fault cases and existing learned text regression. Both image-to-pattern intent controls FAIL; the candidate remains unqualified. Authenticated vision profile/source/preprocessing/report/approval integration and client presentation remain open; existing image generation stays explicitly prompt-conditioned. Roadmap: PARTIAL.


## Authenticated desktop image preparation — 2026-10-11

[Desktop image request review](PULSAR_DESKTOP_IMAGE_ASSISTANT.md) follows [measured owner policy and CLI preparation](PULSAR_VISION_OWNER_POLICY_CLI.md): an imported source is supervised into an exact image context, then saved privately for separate inference and generation approval. Current clients pass 256 tests, both strict workspace lint configurations and the default paired build pass; headless vision controls construct without inference. This closes the scoped presentation implementation, not interactive GUI, useful-model, real-reference, image-conditioned lineage, offline-default or device/release qualification. Earlier shape-to-pattern quality failures remain FAIL. No annotations or training were fabricated. Full roadmap PARTIAL; goal active.

## Retained image-conditioned generation origin — 2026-10-11

[Image provenance](PULSAR_IMAGE_GENERATION_ORIGIN.md) now survives approved pattern dispatch, worker receipt, candidate lineage and required portable object capture through conditional format 13. Engine 843, clients 256 and protocol 130 tests PASS, as do both strict lint configurations and the paired build. The native still/candidate/object-readback case PASS in 102.79 seconds; its separately authored proposal fixture does not establish useful learned output. Full export/import/re-export, historical-reader, UI/export presentation, durability, model/media/platform/device/human/release gates remain open. No annotations or training were fabricated. Full roadmap PARTIAL; goal active.

## Image-origin portable lifecycle — 2026-10-11

[Actual authored export/import/re-export](PULSAR_IMAGE_PORTABLE_LIFECYCLE.md) PASS in 75.34 seconds. Original suppliers are unavailable; two independent clone/restart/re-export/second-clone subjects retain exact record/RGB evidence without executable job authority. Separate record/RGB corruption refuses startup and preserves committed SQLite main/WAL. Default Engine 843 tests, both strict lint configurations and structural guards PASS; final paired build PASS. Historical readers, UI/export presentation, durability and broader model/media/platform/device/human/release gates remain open. Full roadmap PARTIAL; goal active.

## Preceding image-origin reader boundary — 2026-10-11

[Actual frozen-source reader campaign](PULSAR_IMAGE_PRECEDING_READER.md) PASS in 76.53 seconds: compatible package control, format-13 refusal, four cold clone-state refusals with committed SQLite main/WAL preservation and no socket, plus current re-export/restart/corruption checks. Engine 843 and both strict lint configurations PASS; final paired build PASS. The frozen preceding source snapshot is distinct from released historical tags. Broader historical, presentation, durability, useful-model/media/platform/device/human/release gates remain open. Full roadmap PARTIAL; goal active.


## Candidate motion evidence presentation — 2026-10-11

[Candidate review](PULSAR_CANDIDATE_MOTION_EVIDENCE.md) exposes Observed, Predicted, Inferred and Synthesized action counts by supplied axis and separate unresolved-gap interval counts. Synthesized actions and gaps receive visible notices. Client library 258 tests PASS (5 ignored), both strict workspace/all-target lint configurations PASS, architecture 406 and guards 36 PASS. The cache uses exact hydrated program identity and releases discarded motion. This is action-label presentation only: verified retained image-origin details, neutral export provenance, rendered GUI, useful-model/media/training, hardware and release qualification remain open. No milestone is closed.


## Candidate image-origin metadata inspection — 2026-10-11

[Read-only candidate origin inspection](PULSAR_CANDIDATE_IMAGE_ORIGIN.md) now exposes checked original approval, exact retained record/RGB, producer receipt and raw/canonical motion identities through API/CLI/desktop metadata. The actual native campaign PASS in 87.93 seconds, including local response/frame verification, mutation refusals and four original-supplier-free first/second clone inspections with zero jobs/attempts/workers. Independent record/RGB corruption refuses inspection/startup and preserves committed SQLite/WAL. Engine 843 tests PASS (68 ignored), clients 260 (5 ignored), protocol 131, paired build, both strict workspace/all-target lint configurations, format, architecture 408 and guards 36 PASS. Actual CLI grammar/help is checked; shipped IPC and rendered GUI remain UNKNOWN. Pixel preview, richer proposal review, neutral export provenance, useful-model/media/training, physical-device and independent release gates remain open. No milestone is closed.

Retained image pixel increment (2026-10-11): [candidate origin inspection](PULSAR_CANDIDATE_IMAGE_ORIGIN.md) now includes selected approval-bound normalized RGB preview via API/CLI/desktop. Native local and four clone queries PASS (73.62 seconds), with supplier-free retained CAS access, no clone jobs/attempts/workers, altered selector/layout/evidence rejection and independent record/RGB tamper startup refusal preserving SQL/WAL. Observation-backed client decode and headless egui construction PASS for all five frames, including exact RGB/RGBA and corrupt-byte refusal. Default Engine 843, clients 260 and protocol 131 tests PASS; clients have six ignored native tests. Workspace check, paired build, CLI help, format, architecture 410 and guards 36 PASS; both strict workspace/all-target lint configurations PASS, with evidence in the pixel checkpoint. Interactive GUI, shipped CLI IPC, useful-model/media/training, hardware/human/release and remaining roadmap software gates stay open. No milestone is closed.

Synthetic proposal review increment (2026-10-11): [exact bounded sample review](PULSAR_PATTERN_REVIEW.md) adds channel/timing/amplitude/phase/duty review for completed and saved proposals and retained historical image approvals. Shared streamed Core samples preserve generation arithmetic and allocation; dense envelopes include every sample's extrema, never an aliased sparse curve or automatic amplitude stretch. Core 44 + 17 + 2, Engine 843, clients 263 and protocol 131 selected tests PASS; both strict lint configurations, paired build, format, 412 architecture checks and guards 36 PASS. Actual learned-controller IPC checks completed/saved headless graphs, 17 independent candidate/export actions, separate commit, Undo/Redo and stale refusal; owned server is joined. Five preceding local/clone observations pass current retained-pattern/pixel review without claiming a new Engine lifecycle campaign. Neutral evidence export, interactive GUI, model/media/training, default/platform/device/human/release gates remain open. No milestone is closed; goal active.


## Neutral playback and full project association — 2026-10-11

[Motion/evidence verification](PULSAR_MOTION_EVIDENCE.md) now independently checks all native package object hashes, a completed exact export receipt and every selected master playback action. A fresh six-axis authored Engine export verifies offline after Engine termination, with wrong-revision/changed-script refusal and unchanged inputs. Engine/client/protocol selected suites PASS 826/266/152; both strict lint configurations, workspace/all-target check, format, architecture 416 and guards 36 PASS. Standard funscript stays provenance-free. Atomic paired publication, retained recovery identities and native desktop export presentation remain OPEN; this verified prerequisite does not close MOT-07/M4 or broader qualification. No annotations or keys were fabricated. Full roadmap PARTIAL; goal active.


## Paired neutral script and private project export — 2026-10-11

[Paired export](PULSAR_MOTION_EVIDENCE_EXPORT.md) now publishes neutral playback, the full private native project and checked association to one new directory through shared CLI/desktop transfer clients. Durable exact request identities, explicit consent, unchanged grants, conservative interrupted-Begin refusal, held-directory/member checks, no-replace rename/sync and precise recovery outcomes are implemented. Fresh authored six-axis CLI recovery and actual desktop transfer controller PASS with independent 17-action readback, original intent preservation, output modes and unchanged master. Engine/client/protocol suites PASS 826/272/152; both strict lint configurations, check, format, architecture 418 and guards 36 PASS. Creator PackageProject grants remain unchanged. The marker fixture is not actual interrupted-socket qualification; headless rendering is not interactive GUI qualification. Model/media/platform/device/human/release and remaining roadmap gates stay OPEN. Full roadmap PARTIAL; goal active.


## Native desktop bundle conflict and resume — 2026-10-12

The [recovery checkpoint](evidence/roadmap-2026-10-12/desktop-bundle-recovery/checkpoint.json) executes a real pre-publication destination conflict through the shared desktop transfer controller. NotPublished retains the exact stage; the owned blocker and sentinel remain unchanged. Explicitly preserving that blocker and resuming the same intent publishes six scripts with byte-identical intent and preceding authored PCM/timing outputs. Master content/revision remain unchanged. There are six new export reservations for the failed publication and none for resume, with no new generation job/candidate. The new ignored native fixture separately passes. Final client library tests pass 340 with ten ignored, two integration tests pass, both strict workspace lint configurations pass, and Windows source check/native App+Worker build/formatting/architecture/source guards pass. Only the client native test source changes; Engine/Protocol suites were not rerun. Direct controller execution does not qualify interactive GUI, cancellation/death, physical durability, non-Linux runtime or model/media/device/human/release scope. Full roadmap PARTIAL.


## Selected packs for folder generation — 2026-10-12

The [selected-pack checkpoint](evidence/roadmap-2026-10-12/library-packs/checkpoint.json) adds exact owner-selected installed packs to batch and scan --generate through existing single-file pack generation. Native diagnostic commands preserve companions/media, match raw-model outputs/profile identity, create no state on dry/repeat/refusal and enforce deactivation after restart. Serial client tests pass 341 with ten ignored plus two integration tests; both strict lints, Windows source check, native build, formatting and architecture/source guards pass. Initial compile/profile-ordering/lint failures are retained and corrected. The parallel suite has an unresolved cleanup-lock failure (`Cleanup receipt is owned by an active helper`, OS error 11); isolated/serial passes do not close it. See [scope](PULSAR_LIBRARY_PACKS.md). Multi-axis/overwrite/durable folder scheduling, real-media/models/defaults, platform/edition, devices and human/release gates remain open. Full roadmap PARTIAL.


## Cleanup receipt owner lifetime — 2026-10-12

The [lock checkpoint](evidence/roadmap-2026-10-12/cleanup-lock/checkpoint.json) repairs a deterministic inherited-descriptor ownership defect matching the preceding parallel cleanup-lock error. The regression fails before explicit Record unlock and passes afterward while the child remains alive; active owners still block recovery. Parallel client tests pass 342 with ten ignored plus two integration tests; eight additional 32-thread runs pass. Strict lints, Windows source check, native build, formatting and architecture/source guards pass. Actual native cleanup-death/resume preserves the original 5,001 actions and master and joins all owned processes. The original intermittent holder was not captured; its historical FAIL is retained and broader lock/concurrency/fault qualification is not presumed closed. See [scope](PULSAR_CLEANUP_LOCK_OWNERSHIP.md). Other required software and model/media/platform/device/human/release gates remain open. Full roadmap PARTIAL.


## Explicit audio folder bundles — 2026-10-12

The [audio folder checkpoint](evidence/roadmap-2026-10-12/library-audio/checkpoint.json) adds explicit six-axis audio recipes to batch and scan --generate through the existing synthesis and atomic publisher. Native WAV/FLAC results match all prior PCM/timing oracles, avoid same-stem filename collisions, preserve existing companions/targets, and preserve a completed entry plus failed project/job identity on later decode failure. Dry/repeat/eight refusals add no generation state. Single-file atomic, raw-model and selected-pack/deactivation regressions pass. Client tests pass 344 with ten ignored plus two integration tests; strict lints, Windows source check, native build, formatting and architecture/source guards pass. Initial compile and verifier-label failures are retained. See [scope](PULSAR_LIBRARY_AUDIO_BUNDLES.md). Visual six-axis reconstruction, overwrite/durable folder scheduling, other formats/platforms, media/model/creative quality, devices and human/release gates remain open. Full roadmap PARTIAL.


## 2026-10-12 — retained neutral bundle inspection

The [inspection checkpoint](evidence/roadmap-2026-10-12/bundle-inspection/checkpoint.json) adds authenticated read-only local stage inspection against the exact current six-axis master. Missing files are reported; unsafe or mismatched present files refuse. Engine export outcomes remain explicitly unverified even when all local files match. Actual network-disabled CLI cases preserve files and database counts; the native exact-intent conflict/resume regression still matches preceding PCM/timing oracles. Client tests pass 346 with ten ignored plus two integration tests; strict lints, Windows source check, native build, formatting and architecture/source guards pass. Three client source paths change. See [scope](PULSAR_NEUTRAL_BUNDLE_INSPECTION.md). Interactive GUI, durable folder scheduling, broader platform/durability, model/media/device/human/release and remaining roadmap gates stay open. Full roadmap PARTIAL; goal active.


## 2026-10-12 — retained local batch generation

The [local ledger checkpoint](evidence/roadmap-2026-10-12/library-ledger/checkpoint.json) adds Engine-owned immutable audio intake, owner-scoped list/status and explicit exact-ID generation advancement. Native changed/deleted originals and a changed recipe still produce twelve scripts matching preceding PCM/timing oracles after restart; repeat adds no jobs. The older reader refuses without database change. Lost create acknowledgement, source corruption, ownership, eviction and first-worker barrier regressions pass. Validation repairs inner schema publication that could drop the outer barrier, and benchmark profile/actual-manifest correspondence. Failures are retained. Final default libraries pass Engine859/Clients346/Protocol160 with70/10/0 ignored, and selected integrations, both strict lints, Windows source/native build/formatting/architecture guards pass. See [scope](PULSAR_LOCAL_BATCH_GENERATION.md). Ordinary batch/scan stay unchanged; aggregate publication, migration/cancel/release and broader fault/GUI/platform/model/media/device/human/release qualification remain open. Full roadmap PARTIAL; goal active.


## 2026-10-12 checkpoint: local batch metadata release

Explicit authenticated release frees unstarted or generation-ready ledger membership and pins, with an atomic exact-request receipt. Partial generation, pending acknowledgements and item errors refuse release. Source/media/output files, downstream project/job/export records and other batch pins remain preserved; no publication or qualification claim follows. See [ADR B3-164](../research/phase-b/ADR-B3-164-local-batch-release.md) and [scope](PULSAR_LOCAL_BATCH_GENERATION.md). Full roadmap PARTIAL; aggregate publication scheduling and model/media/platform/device/human/release gates remain open.


## 2026-10-12 checkpoint: retained local batch publication handoffs

Explicit client prepare/publish/status connects retained generation to the existing neutral publisher. A private journal retains exact stage/export request and parent/stage filesystem identities before effects, and actual publisher receipts per entry afterward. Unknown acknowledgements refuse replacement/adoption; generation and publication history remain distinct. See [ADR B3-165](../research/phase-b/ADR-B3-165-local-batch-publication-handoff.md) and [scope](PULSAR_LOCAL_BATCH_PUBLICATION.md). Full roadmap PARTIAL; unified scheduling, unresolved acknowledgement disposition, model/media/platform/device/human/release gates remain open.


## 2026-10-12 checkpoint: bounded retained local batch scheduling

Explicit run/status connects captured input, retained generation and per-entry publication. Private schema-2 history is stored before generation; immutable/acquired identities cannot regress or change. Ready entries publish before later generation, read-only job observation avoids durable poll replies, and failures preserve earlier receipts without replacement jobs. See [ADR B3-166](../research/phase-b/ADR-B3-166-local-batch-scheduler.md) and [scope](PULSAR_LOCAL_BATCH_SCHEDULER.md). Full roadmap PARTIAL; continuous watching/intake, cancellation/disposition/migration/GC, unknown acknowledgements and model/media/platform/device/human/release gates remain open.


## 2026-10-12 checkpoint: durable local batch cancellation and partial metadata release

Explicit owner-scoped cancellation durably stops new batch generation, retains exact known-job cancellation identities and reconciles pending generation only from existing positive cache receipts. Unknown acknowledgements remain pending; no replacement work or output rollback is supplied. Completed cancelled partial batches can release only metadata memberships after terminal-job and absent-worker-handle checks. Scheduler/publication observe stop policy and retain prior receipts. The outer reader floor is 29; inner storage writers preserve it. See [ADR B3-167](../research/phase-b/ADR-B3-167-local-batch-cancellation.md), [contract](PULSAR_LOCAL_BATCH_CANCELLATION.md) and [checkpoint](evidence/roadmap-2026-10-12/library-cancellation/checkpoint.json).

Selected-package tests pass 1,522 with 80 ignored; final clients, both strict lints, source/platform/build/format/architecture checks and native cancellation/scheduler campaigns pass. The older reader refuses and preserves database/WAL bytes; its shared-memory sidecar preservation assertion remains FAIL. Cancellation acknowledges software stop, not physical worker retirement. Full roadmap PARTIAL; unknown-receipt disposition, migration/collection, continuous watching/intake and model/media/GUI/platform/device/durability/human/release gates remain open.


## 2026-10-12 desktop retained audio library checkpoint

The desktop Library batches tab now exposes retained audio-recipe intake, owned catalog/status, shared bounded generation/publication, local pause between decisions, exact mutation recovery, durable cancellation, metadata release and retained project opening. Native controller IPC and the final CLI scheduler regression pass; twelve outputs match retained PCM/timing oracles. Client tests pass 366 with ten ignored; both strict lint configurations and remaining scoped checks pass. Headless egui tests do not establish rendered GUI acceptance. Visual/model folder controls and all independent platform/model/media/device/human/release gates remain open. See [ADR B3-168](../research/phase-b/ADR-B3-168-desktop-library-batches.md), [contract](PULSAR_DESKTOP_LIBRARY_BATCHES.md) and [checkpoint](evidence/roadmap-2026-10-12/desktop-library/checkpoint.json).


## 2026-10-12 shared desktop audio recipe authoring

The retained desktop audio workflow now uses the same per-axis recipe controls as single-file Audio generation, with explicit JSON intake retained. Direct capture snapshots checked settings before background discovery and retains full original options for exact recovery; local discovery failures remain editable. Both native editor/file workflows pass and all 24 exported scripts match preceding authored PCM/timing oracles. Client tests pass 370 with ten ignored; both strict lint configurations and remaining affected checks pass. Rendered GUI acceptance, visual/model folder workflow, broader input/edition integration and independent model/media/platform/device/human/release gates remain open. See [ADR B3-169](../research/phase-b/ADR-B3-169-library-audio-recipe-controls.md), [contract](PULSAR_LIBRARY_RECIPE_AUTHORING.md) and [checkpoint](evidence/roadmap-2026-10-12/library-recipe-controls/checkpoint.json).


## 2026-10-12 original batch preparation recovery

Explicit CLI preparation recovery now binds the exact retained stage to its original private pending receipt and the current journal transition. It restores Prepared metadata without generation/export dispatch; missing or mismatched original evidence refuses. Actual client death after pending-receipt fsync and before record replacement, exact repeat/restart and four native refusals pass; twelve outputs match retained PCM/timing oracles. Client tests pass 373 with ten ignored; both strict lint configurations and remaining affected checks pass. Desktop recovery controls, broader faults and independent qualification remain open. See [ADR B3-170](../research/phase-b/ADR-B3-170-original-batch-preparation-recovery.md), [contract](PULSAR_LIBRARY_PREPARATION_RECOVERY.md) and [checkpoint](evidence/roadmap-2026-10-12/library-preparation-recovery/checkpoint.json).


## 2026-10-12 desktop preparation recovery

The desktop library controller now exposes exact original preparation recovery with captured batch/directory/index/stage/receipt inputs and fresh history. Known preflight refusals remain editable; uncertain metadata writes retain the original action and block replacement mutations. Actual client-death recovery, owned read-only write refusal/exact retry and desktop replay after Engine restart pass; twelve outputs match retained timing oracles. Client tests pass 375 with ten ignored; both strict lint configurations and remaining affected checks pass. Rendered GUI acceptance and broader independent gates remain open. See [ADR B3-171](../research/phase-b/ADR-B3-171-desktop-preparation-recovery.md), [contract](PULSAR_DESKTOP_PREPARATION_RECOVERY.md) and [checkpoint](evidence/roadmap-2026-10-12/desktop-preparation-recovery/checkpoint.json).
