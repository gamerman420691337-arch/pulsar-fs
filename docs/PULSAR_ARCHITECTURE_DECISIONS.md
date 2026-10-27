# Pulsar Architecture Decisions and Design Continuation

Version: 0.3
Date: 2026-09-11
Status: Architecture discussion complete; implementation and qualification unfinished

## 1. Purpose, authority, and current work status

This document preserves the architecture discussion following the product interview, including explicit decisions, the inspected source structure, proposed module responsibilities, verification goals, and questions still to resolve. It is the continuation record for further interface design, not an implementation-complete specification.

Read these documents together:

- [Software specification](PULSAR_SOFTWARE_SPECIFICATION.md): existing product requirements and the earlier interview's decision traceability, including modalities, quality/speed, platforms, packaging, assistant, editing, and devices.
- [Major roadmap](PULSAR_MAJOR_ROADMAP.md): M1-M8 sequencing and evidence-bound milestone closure.
- This record: subsequent architecture decisions and unresolved interface design. The explicitly selected decisions below extend the earlier architecture direction; proposals do not become requirements merely by appearing here.

The product specification remains authoritative for existing product requirements. Subsequent explicit architecture choices are recorded here rather than silently rewriting the older specification. Any actual conflict must be surfaced to the product owner before implementation; numeric product gates are not redefined here.

Status labels:

| Label | Meaning |
| --- | --- |
| Confirmed | Explicit user statement or submitted structured choice. |
| Proposed | Engineering recommendation discussed but not independently finalized. |
| Observed | Dated source or test evidence; not proof of release qualification. |
| Open | Further design, investigation, or human decision required. |

The user requested a full bug, architectural purity, and correctness pass, but first requested discussion of actual source structure and joint design of interfaces. Implementation was paused for that design. This documentation write does not resume implementation, authorize device actuation, or declare any milestone complete.

The user explicitly requested durable documentation and an explicit statement when architecture and planning are finished. Completion criteria appear in Section 11. Until they are satisfied, report planning as incomplete.

## 2. Confirmed user direction beyond the menu choices

- Modernize legacy and misleading names. `FunGenApp` prompted a source-origin concern; changing a name alone neither proves nor disproves implementation origin.
- Harden the design architecturally and technologically, including stronger types and explicit interfaces.
- Preserve provenance across interfaces where needed and remove unnecessary detail deliberately when no longer needed.
- Make modules independently programmable and testable through their interfaces, while checking whole-program composition.
- Include mutation testing, known-answer tests (KATs), fuzzing, and additional verification techniques.
- Seek rigorous correctness assurance, with the formal scope subsequently narrowed by the explicit critical-path decision below.
- Discuss and settle architecture before undertaking the requested broad implementation/bug pass.

These are design objectives, not assertions that the existing implementation already provides them.

## 3. Confirmed architecture decision ledger

Rows ARCH-001 through ARCH-013 retain the exact structured question IDs used in the architecture interview. Subsequent rows identify their follow-up question or direct user statement. Only submitted answers are treated as decisions; no decision is inferred from a preselected option.

| ID | Decision source | Submitted answer | Confirmed direction |
| --- | --- | --- | --- |
| ARCH-001 | `generation_process_isolation` | Worker processes (Recommended) | Decoding and inference execute in worker processes. GUI and CLI share one typed application interface. Worker launch remains compatible with the portable distribution. |
| ARCH-002 | `headless_engine_lifetime` | Continue headlessly (Recommended) | A headless engine owns jobs and project state independently of the GUI. Offline generation continues after GUI closure/crash, and GUI or CLI can reconnect. Engine crash recovery details remain open. |
| ARCH-003 | `media_editor_scope` | Motion + analysis (Recommended) | Edit motion and non-destructive media trims, crops, regions, and timing alignment. Preserve original media. A full video/audio composition-and-export editor was not selected. |
| ARCH-004 | `source_lineage_strategy` | Audit then refactor (Recommended) | Establish code/dependency origins and attribution, then retain or replace modules based on findings. Do not claim clean-room status without evidence. An independent whole-program reimplementation was not selected. |
| ARCH-005 | `provenance_retention_default` | Compact audit graph (Recommended) | Native projects retain source/model/configuration/transform lineage and traceable observations for generated motion and review flags. Heavy raw/intermediate traces are opt-in. Exact schema, granularity, retention, and size budgets remain open. |
| ARCH-006 | `assistant_edit_autonomy` | Scoped automatic edits (Recommended) | Permit undoable assistant edits within authorized, unprotected project scope. Protected replacements, external effects, and device actions require separate authorization. Grant lifetime and detailed permissions remain open. |
| ARCH-007 | `playback_controller_loss` | Explicit headless mode (Recommended) | Loss of the controlling GUI/CLI stops physical playback by default. Continuing requires explicit prior headless-session authorization. This is separate from permission to continue offline generation. |
| ARCH-008 | `formal_assurance_depth` | Critical-path proofs (Recommended) | Require formal models/proofs for authorization, revision transactions, job/device lifecycles, and key invariants. Use strong numerical and empirical testing elsewhere. A proof-first rewrite of the entire deterministic core was not selected. |
| ARCH-009 | `extension_execution_scope` | Sandboxed executable plugins | Include third-party executable plugins in the initial hardened architecture. Do not defer executable extensions in favor of built-ins and data packs only. |
| ARCH-010 | `initial_plugin_roles` | Also runtimes and drivers | Plugin roles include third-party inference backends and device-protocol implementations, not only analysis and authoring. Different roles need distinct trust and admission contracts. |
| ARCH-011 | `third_party_driver_qualification` | Qualification gate (Recommended) | Normal playback requires accepted driver/version qualification before real device-control authority. Development and physical qualification use a separate explicitly authorized test mode. Unqualified experimental playback was not selected as the normal alternative. |
| ARCH-012 | `engine_protocol_support` | Versioned local protocol (Recommended) | Support a versioned local engine protocol for third-party clients as well as bundled GUI/CLI. Define command/event schemas and compatibility rules; enforce capabilities in the engine. This does not authorize a remotely exposed control server. |
| ARCH-013 | `project_revision_conflicts` | Reject stale commits (Recommended) | Project mutations require the matching project revision. Preserve stale generation/edit results as candidates for explicit review/rebase instead of overwriting newer work. Automatic proven-disjoint merging was not selected. |
| ARCH-014 | Follow-up question: one master timeline versus multiple independent timelines per project | "One master timeline, yep." | Each project has one master motion timeline, with aligned media/analysis inputs, motion axes, and one revision history. Libraries and batches coordinate multiple projects rather than introducing independent timelines inside a project. Concrete timebase and alignment contracts are finalized in CONTRACT-02 and CONTRACT-04 below. |
| ARCH-015 | Follow-up question: one engine per OS user versus one engine per project | "one engine per os user" | One headless engine per OS user coordinates projects, shared compute budgets, and device ownership. Project permissions, revisions, and assistant contexts remain separately scoped. Decoding/inference workers remain isolated; engine sharing does not grant cross-project authority. Startup, discovery, scheduling, and recovery contracts are finalized in CONTRACT-05 below. |
| ARCH-016 | `project_time_authority` | "Independent project time" | Each project has an independent time domain. Source timestamps map onto project time; project time is not defined by a primary video. Playback scheduling uses a separate monotonic clock. Concrete units, mappings, discontinuities, and rounding contracts are finalized in CONTRACT-02 below. |
| ARCH-017 | `source_alignment_motion_policy` | "Explicit retime" | Source alignment changes preserve existing motion and flag affected derived results as stale. Retiming or regeneration requires an explicit operation. |
| ARCH-018 | `project_edit_durability` | "Durable commits" | Persist each committed edit transaction before reporting success. Interactive previews remain transient. |
| ARCH-019 | `project_undo_scope` | "Latest project edit" | One shared, actor-labelled project undo history. Undo reverses the latest committed edit regardless of originating client. |
| ARCH-020 | `client_playback_scope` | "Independent previews" | Clients have independent preview playheads over the same project timeline. Clients can attach to engine-owned device playback, which has one exclusive controller. |
| ARCH-021 | `running_job_revision_change` | "Finish snapshot job" | Project edits do not automatically cancel running generation. Jobs finish against immutable input snapshots; old-revision results remain candidates for explicit review/rebase and cannot silently commit over newer edits. |
| ARCH-022 | `interrupted_offline_job_recovery` | "Resume eligible jobs" | When the engine next starts after a crash, automatically resume eligible interrupted offline generation from validated checkpoints with unchanged inputs/configuration. Missing or changed dependencies block recovery. This does not authorize OS-login autostart or device playback. |
| ARCH-023 | `third_party_client_authority` | "Pair and scope" | Third-party local clients require explicit pairing and grants scoped to projects and operations. Device and external-effect permissions remain separate. This is not protection against a fully compromised same-user OS account. |
| ARCH-024 | `plugin_isolation_failure_policy` | "Allow trusted exception" | If required confinement is unavailable, the user may explicitly authorize unsandboxed third-party plugin execution. Warn that access may exceed project grants; the assistant cannot enable this exception. Such plugins enter the trusted computing base and are excluded from sandbox guarantees. Driver qualification remains separately required. |
| ARCH-025 | `unsandboxed_plugin_approval_scope` | "Exact build" | Unsandboxed plugin approval is bound to the exact artifact identity/hash. Changed plugin bytes require fresh user consent. Driver qualification remains a separate build-specific gate. |
| ARCH-026 | `headless_playback_stop_assurance` | "Require bounded stop" | Explicitly authorized headless device playback requires qualified device-side behavior that bounds residual motion after engine or connection failure. Unsupported device/transport modes remain attended-only. A stop command alone does not establish physical stopping. Acceptable deadlines follow the per-device approval policy. |
| ARCH-027 | `headless_stop_deadline_policy` | "Per-device approval" | Each qualified device/firmware/transport configuration declares its failure-stop bound. Explicit user approval of that bound is required before headless use; no universal numeric deadline is selected. Qualification and approval must correspond to the configuration actually used. |
| ARCH-028 | `first_release_motion_output_cardinality` | "One program initially" | First release supports one device-neutral motion program per project, with up to six axes. Multiple sources/entities may inform it. Explicit motion-target and motion-output identities preserve future expansion. This does not decide simultaneous physical-device playback. |
| ARCH-029 | `viewer_uncached_observation_behavior` | "On-demand preview" | Viewer uses frame-matched cached observations, otherwise requests lightweight preview analysis. Stale boxes remain hidden until results match the displayed frame and current source/mapping context. Rendering remains independent of generation completion. |
| ARCH-030 | `job_media_snapshot_policy` | "Snapshot media" | Generation uses immutable engine-managed media snapshots. Use copy-on-write snapshots when supported, otherwise copies. Preparation time and storage are explicit; jobs block when required snapshots cannot fit approved disk budgets. Snapshot identity is content-bound, not merely a mutable source path. |
| ARCH-031 | `completed_media_snapshot_retention` | "Evict with explicit pinning" | After active work no longer requires them, unpinned media snapshots may be evicted. Preserve committed motion, compact lineage, and source content identities. Exact reruns then require matching originals or restored snapshots. Users may pin/package projects for self-contained replay. Do not treat original media or committed project state as evictable cache. |
| ARCH-032 | `capability_revocation_running_work` | "Stop affected work" | Revoking a client/plugin grant blocks new commands, cancels work authorized solely by it, and withdraws associated device-session authority with stop/disarm handling. Preserve committed edits/history. Late worker results cannot regain revoked authority. This differs from ordinary client disconnection. |
| ARCH-033 | `automatic_backend_failure_policy` | "Equivalent fallback only" | When an automatically selected backend fails, a new recorded attempt may use a qualified equivalent that preserves requested quality tier. Disclose backend/performance changes; never silently lower tier or reuse incompatible checkpoints. Explicit backend pins remain strict. Existing crash-resume identity requirements still apply. |
| ARCH-034 | `unqualified_model_execution_scope` | "Allow labelled custom runs" | Structurally validated user-imported models/backends may run in ordinary projects with prominent unqualified status and no official speed/quality claims. Such imports do not authorize executable custom code, broaden grants, bypass confinement policy, or bypass driver qualification. Qualified built-in tier selection cannot silently become an unqualified custom run. |
| ARCH-035 | `driver_qualification_acceptance_authority` | "Official or local qualification" | Normal playback may use approved bundled qualification records or explicit user acceptance after required local qualification. Bind acceptance/evidence to exact driver artifact, device, firmware, and transport configuration; required evidence remains mandatory. Assistant execution of checks does not authorize acceptance. Headless stopping-bound approval remains separately required. |
| ARCH-036 | `formal_gate_release_stage` | "Gate Preview" | First public Preview requires the defined critical-path lifecycle/model-checking and bounded Rust proof obligations to pass alongside tests. Proof bounds, assumptions, implementation correspondence, and trusted components remain explicit. This does not certify neural quality or physical hardware. |
| ARCH-037 | `assurance_campaign_cadence` | "Layered gates" | Run fast contract/KAT/property and targeted critical proof checks per PR. Run deeper fuzz, mutation, backend, and hardware campaigns for affected surfaces and release candidates. Results must bind to their actual subjects/configurations; successful fast gates do not waive required deeper gates. |
| ARCH-038 | `legacy_public_interface_migration` | "Compatibility window" | Modernize internal names now while preserving existing documented CLI syntax/aliases and non-destructive project import/migration through the first Stable major series. Remove deprecated public interfaces only in a subsequent major with a migration path. Preserve standard funscript interoperability permanently; compatibility does not preserve known defects. |
| ARCH-039 | `active_playback_revision_updates` | "Explicit switch" | Physical playback remains pinned to its admitted motion revision. Ordinary project edits do not alter active hardware commands. A controller with device authority must explicitly request a revision switch; re-admit the new motion and stop/rearm when the transport cannot switch safely. Authorized live-input generation is a distinct session mode, not implicit permission for ordinary edits. |
| ARCH-040 | `assistant_unsandboxed_extension_invocation` | "Approve each invocation default with a second explicitly risky auto mode/standing grant." | Exact-build load approval does not automatically permit assistant invocation. Default requires approval for each proposed assistant call. The user may separately enable a clearly labelled risky automatic mode/standing grant for that exact build and declared call scope. Assistant cannot enable the exception or risky mode; revocation and changed plugin bytes invalidate authority. Native effects remain outside sandbox guarantees. |
| ARCH-041 | `architecture_first_delivery_sequence` | "First we fix the architecture completely into the final shape, scaffolding everything. Then we make it good, then we make it solid." | Supersedes the prior M1-before-architecture delivery order. Phase A establishes the final four-library ownership/dependency shape and real composed paths; Phase B solves functional/technical challenges through evidence-led research and correctness work; Phase C completes hardening and qualification. Preserve M1-M8 acceptance scope, historical defects, product requirements and human authority. ADR decisions link reproducible experiment evidence; structural integration is not qualification. |

No menu choice selected a particular inference runtime, sandbox technology, IPC serialization, formal verification tool, native project schema, or exact crate layout.

## 4. Original module proposal and how discussion refined it

The user's proposed starting structure was:

1. CLI/orchestration, with GUI conceived as a visual extension of a CLI running in a background terminal.
2. Media input and parser.
3. Media viewer.
4. Media editor.
5. DSP: general-purpose DSP, optical flow/Lucas-Kanade, and neural imperative/functional work including runtime, model parsers/detectors, model zoo, and bounded local chat assistant.
6. Hardware validation doctor.
7. Internal test suite.

The discussion retained these capabilities but proposed different ownership:

- GUI and CLI should be peers using the engine interface, not a terminal screen-scraping relationship. Worker isolation and a reconnectable headless engine were explicitly selected.
- Separate media parsing/decoding from viewing, project editing, and playback-clock coordination.
- Separate numerical DSP, perception semantics, model execution, model artifact management, and assistant authority. A single DSP module should not own all of them.
- Add explicit device-command admission and session ownership. A diagnostic doctor cannot substitute for policy enforced on every output path.
- Keep the core motion/project domain shared by editing, processing, playback, and extensions.
- Treat tests as consumers of module interfaces and composition contracts, not as another production-state owner.
- Add an Extensions module because executable plugins, including runtimes and drivers, were explicitly selected.

These logical responsibilities guide the next discussion. Exact module/crate boundaries and interfaces are still open.

## 5. Observed source structure and defects motivating the design

### 5.1 Inspection baseline and limits

Inspected upstream: `7a4f5671c14731b238b64c4827294ce029928dda`.

Roadmap/specification commit: `43260c3165fcf9ddd5b961f0fb58d320268c0265` on `codex/pulsar-roadmap-m1`, in a separate worktree. The older divergent checkout at `434b830c2c7d004d04830d299e8c5159d43b047e` was preserved.

The earlier remote refresh, including the check after the user requested the latest pushed version, still resolved `origin/main` to the inspected baseline. This is historical evidence, not a permanent claim about current upstream.

The inspected package is Rust `pulsar` version `0.8.0`, with a binary entry point in `src/main.rs` and no separate library entry point in the inspected tree. Its manifest declares `AGPL-3.0-only`; no licensing change is authorized by this record.

Source mapping was targeted, not an exhaustive call-graph or runtime audit. Some larger modules were inspected at signature level. Absence of a safeguard from an inspected interface does not prove that no caller implements it elsewhere. These observations were not revalidated for this documentation-only write.

### 5.2 Current modules and interfaces

| Current source | Observed responsibility and interface |
| --- | --- |
| `src/main.rs` | CLI and generation orchestration; directly configures media, flow/neural work, action extraction, multi-axis assembly, and export. |
| `src/gui/app.rs` | `FunGenApp`, application/presentation state, editing/playback integration, Cinema rendering, and a separate background generation loop. `WorkerMessage` carries progress/status or a completed `MultiAxisScript`, not tracking boxes. |
| `src/gui/timeline.rs` | Timeline state, coordinate conversion, selection, zoom/pan, loops, bookmarks, and timeline rendering tied to egui and funscript types. |
| `src/gui/live_recorder.rs` | Arming, recording/simplifying takes, and splicing recorded actions into a script. |
| `src/gui/rig_simulator.rs` | Presentation of rig geometry using kinematics types. |
| `src/video.rs` | Media probing, extraction and streaming, timestamps, RGB/gray conversion, and playback-related helpers. Exposes `FrameStreamReader`, `StreamConfig`, and `extract_frame_at`. |
| `src/audio.rs`, `src/audio/spectral.rs` | Audio-related implementation exists; detailed ownership and all call sites remain to be mapped. |
| `src/tracking.rs` | Dense flow, scene-cut/motion-center calculations and projections; exposes `FlowField`, `FlowScratchContext`, and dense-flow/projection functions. |
| `src/neural/mod.rs` | ONNX session construction and `NeuralDetector` entry points. |
| `src/neural/yolo.rs`, `src/neural/yolo26.rs` | Preprocessing, legacy/direct tensor decoding, detection types, layout/configuration choices, and related mask assembly. Family names alone do not establish compatibility with actual model exports. |
| `src/neural/tracker.rs` | `AnatomicalTracker`, `TrackedPoint`, `TemporalMemoryBank`, detection selection, occlusion state, and pose estimation. |
| `src/neural/point_tracker.rs` | Sparse trajectory state, seeding, flow propagation, and an optional neural path. Its algorithm branding is not evidence of a qualified learned tracker. |
| `src/neural/router.rs`, `src/neural/hypothesis.rs`, `src/neural/pose.rs`, `src/neural/pipeline.rs` | Confidence routing, alternative motion interpretation, landmark-to-motion mapping, and another adaptive orchestration layer. Further ownership separation is required. |
| `src/neural/model_manager.rs`, `src/neural/sam.rs` | Model discovery/registry/artifact and segmentation-related surfaces; actual capability and model qualification remain separate. |
| `src/signal.rs` | Integration, detrending, normalization, and action extraction using numeric vectors and timestamp slices. |
| `src/funscript.rs` | `Action`, `Funscript`, `AxisChannel`, `MultiAxisScript`, interpolation, sanitization, diagnostics and export-related behavior. |
| `src/batch/queue.rs`, `src/batch/detector.rs` | Batch jobs/configuration/progress, a threaded worker with media/flow/signal dependencies, and library coverage/audit/enqueue operations. Full parity with GUI/CLI is not established. |
| `src/kinematics/device.rs`, `scurve.rs`, `rig.rs`, `thermal.rs` | Device profiles and limits, kinematic state, curve smoothing, rig solutions, and a thermal model. Calculated thermal load is not measured hardware temperature. |
| `src/sync/tcode.rs`, `serial.rs`, `buttplug.rs`, `handy.rs`, `vr.rs` | Concrete protocol/transport/session implementations, including UDP, serial, Buttplug, Handy HTTP, and incoming VR telemetry. Interfaces vary in command shape, errors, events, and lifecycle. |
| `src/plugin/engine.rs` | In-process `FunscriptPlugin: Send + Sync` registry with metadata/parameters/action transformation and three built-ins. This interface is not a security sandbox. |
| `src/stash/client.rs` | Synchronous HTTP/GraphQL integration with queries and remote mutations such as tagging/scanning. External path strings need separate local resolution/authorization policy. |

Current generation shape, simplified:

```text
CLI generation loop ----+
                       +--> media frames --> optical flow / neural detector + tracker
GUI generation loop ----+                                      |
                                                        samples / poses
                                                               |
                                                        signal processing
                                                               |
                                                        script channels
                                                               |
                                                        export / playback

Batch worker has its own media/flow/signal orchestration dependencies.
```

Observed architectural pressure includes duplicated orchestration, weakly distinguished numeric/coordinate/time values, overlapping temporal state, and presentation disconnected from measured observations. These motivate design; they do not imply every proposed replacement has already been approved.

### 5.3 The reported stationary GUI box

The user reports a box locked at one coordinate across all attempted videos, without a failing clip to supply.

At the inspected baseline, `render_tracking_overlay` in `src/gui/app.rs` starts around line 795. It fixes the reticle's horizontal coordinate at video center and derives vertical position from funscript stroke percentage. Around lines 835-838, it creates a rectangle centered on `video_rect.center()`, with width `min(video_rect.width() * 0.40, 320.0)` and height `video_rect.height() * 0.75`. The Cinema caller uses the script's interpolated playback position.

This is a viewport-centered ROI guide, not a detector-driven bounding box. The existing worker message interface does not deliver bounding-box telemetry. Fixing backend propagation alone cannot make this rectangle follow media content.

The source-level explanation is confirmed for that overlay. Reproduction against the user's running build and representative media remains separate. Required future design work includes source/time/coordinate-qualified observations reaching the viewer, visible handling of unavailable or stale tracking, and distinct labeling of simulation versus measured tracking. Exact preview/analysis behavior is still open.

### 5.4 Prior debugging evidence, not completed fixes

Before architecture planning paused implementation, the existing suite passed 130 tests. Twenty added regressions failed on an isolated copy of the pinned upstream, before production source changes:

| Area | Count | Reproduced problems |
| --- | --- | --- |
| Legacy decoder | 5 | Negative extents, non-finite inputs, clipped-center inconsistency, unknown-class panic, zero-class candidate. |
| Direct decoder | 3 | Reversed corners, guessed border/center format, non-finite score. |
| Temporal tracking | 4 | Center moves without box, stale state across scene cuts, confidence ordering replacing spatial continuation, missing detections not aging without flow. |
| Sparse points | 3 | Degenerate box seeding, zero-count mask division, forward flow reused as backward flow. |
| Signal extraction | 4 | Flat input inventing a stroke, reversed decreasing motion, trailing pause gaining a reversal, lost plateau boundary. |
| Flow scratch | 1 | Equal-area resolution change retaining old grid dimensions. |

Baseline commands were `cargo test --locked --bin pulsar -- --nocapture` and a targeted `cargo test --locked --manifest-path /tmp/pulsar-m1-baseline.hdP72q/Cargo.toml --bin pulsar m1_ -- --nocapture`. The latter returned exit 101 with 0 passed, 20 failed, and 130 filtered out. The temporary path is historical local evidence, not a guaranteed durable fixture location.

The local FunGen-12n-pov-1.1.0 model and hand-only animation fixture were present for the earlier smoke tests. Observed model shapes were `[1, 3, 640, 640]` input and `[1, 14, 8400]` output. These are smoke observations, not quality or overlay qualification.

Prepared backend edits were not applied. Their transient preparation is not a durable implementation artifact or a completed milestone. Reconcile any future implementation with the approved interfaces and current baseline rather than blindly applying an earlier patch. The full requested bug/correctness pass remains pending.

## 6. Proposed target modules and ownership

The selected process direction is:

```text
GUI / CLI / assistant controller / third-party clients
                         |
             versioned local engine protocol
                         |
                  headless engine
       projects / permissions / jobs / playback
                         |
           isolated workers and qualified adapters
        media / inference / executable plugins / devices
```

This is a logical ownership diagram, not a finalized process topology. It does not choose one engine per user versus per project, a worker pool size, or the physical location of every adapter. One portable distribution does not require one process, and a background terminal is not the engine protocol.

| Proposed module | Owns | Must not silently absorb |
| --- | --- | --- |
| Domain | Motion/project/source identities, revisions, units, coordinate/time distinctions, and invariants. | GUI types, filesystem operations, inference sessions. |
| Application | Typed commands/events, job coordination, transactions, cancellation, capability enforcement. | Rendering or detector-specific numerical implementation. |
| Media | Probe/parse/decode and source timestamp/projection metadata. | Editing authoritative project state or synthesizing motion. |
| Project editing | Undoable edits, protected regions, regeneration application, revision conflicts. | Direct device control or destructive source-media editing. |
| Playback | Clock authority, seek/pause/rate, synchronization and scheduling. | Independent generation algorithms. |
| Perception | Optical flow, detection, tracking and observation fusion. | Treating inferred device positions as measured physical truth. |
| Motion processing | General DSP, synthesis, neutral-axis motion and explicit creative transforms. | Model installation or unrestricted external effects. |
| Inference | Model execution, backend adapters, tensor contracts and resource accounting. | Project commands or permission decisions. |
| Model catalog | Manifests, artifact identities, capabilities, compatibility and artifact lifecycle. | Deciding whether model observations are correct. |
| Device execution | Profile adaptation, command admission, arming, deadlines, stop/reconnect and transport coordination. | Rewriting the neutral master or delegating all safety to diagnostics. |
| Diagnostics | Capability probes, validation reports and reproducible evidence. | Bypassing normal device authority outside explicit test mode. |
| Extensions | Role-specific plugin interfaces, grants, budgets, lifecycle, provenance and qualification integration. | One unrestricted host interface for all extension classes. |
| Clients and integrations | GUI, CLI, assistant controller and external integration adapters. | Separate generation pipelines or independent project truth. |
| Persistence and distribution adapters | Durable project/artifact storage and portable pack lifecycle behind explicit interfaces. | Hidden mutation or ambient authority in pure computation. |

Module names and crate placement are proposals. Use separate crates where compiler-enforced dependency direction is valuable, not automatically one crate per table row. Favor deep modules: substantial behavior behind a small interface, with implementation choices hidden from callers. Avoid pass-through abstraction layers that merely move complexity around.

Pure numerical functions and explicit state machines should be separated from effectful adapters. Stateful tracking, runtime sessions, and resource scheduling should not be mislabeled pure merely to satisfy an architectural slogan.

GUI presentation state, authoritative project state, and worker execution state require distinct ownership. The proposed split leaves viewport/layout rendering with the GUI while the engine owns semantic application state. Exact selection, navigation, preference, and assistant-to-GUI intent interfaces remain open.

## 7. Interface, type, and provenance design agenda

For every module, specify the complete interface: owned state, accepted values, output guarantees, units, identities, ordering, errors, cancellation/reset, concurrency, performance/resource behavior, and allowed side effects. A type signature alone is not a sufficient contract.

Candidate distinctions discussed, with exact Rust types and wire schemas still open:

- Source identity, project revision, job/attempt identity, and result lineage.
- Media timestamps versus monotonic scheduling instants, including seek/cut epochs and timestamp conversion.
- Pixel versus normalized coordinates and the identity of the image/projection space to which they belong.
- Observed, inferred, synthetic, unavailable, and unresolved information without fabricated certainty.
- Neutral motion versus device-specific, admitted commands.
- Immutable source references and reversible analysis transforms, including mapping observations back through crops/trims/time alignment.
- Generation/edit candidates versus committed project revisions.
- Authorized capabilities versus a model's requested operation or a plugin's declared desire for access.

Potential interface work packages are client commands/results, job events/checkpoints, media frames/buffer ownership, observations, motion candidates, edit transactions, admitted device commands, and plugin host operations. These are not finalized method names or schema definitions.

Provenance design recommendations:

- Carry compact references through hot paths instead of copying full lineage into each tensor or sample.
- Keep detailed lineage in project-associated records and retain enough linkage to explain motion and localized review flags.
- Treat metadata stripping as an explicit projection to a different artifact purpose, such as playback export, rather than deletion of native project history.
- Do not let stripped artifacts masquerade as fully traceable originals or discard identities required for permission, revision, freshness, or device checks.
- Distinguish reproducibility metadata from a promise that missing source/model artifacts or nondeterministic backends can be replayed exactly.

Which records are mandatory, how references are validated, retention/garbage collection, privacy redaction, and the lifecycle of optional heavy traces remain open.

## 8. Authority, isolation, and source-origin constraints

### 8.1 Assistant and client authority

The discussed security direction is an engine-enforced, capability-scoped command interface. The assistant must not receive unrestricted shell or filesystem authority. If it emits CLI syntax, parse that syntax into allowlisted application operations rather than executing a shell command string.

The policy-enforcing module validates requests independently of model output. Scoped automatic edits remain undoable and subject to project revision and protection rules. GUI navigation can be expressed through bounded application/view intents rather than arbitrary mutation of GUI internals. Exact grants, authenticated client sessions, approval persistence and revocation are unresolved.

### 8.2 Workers and extensions

Process isolation is crash containment, not a complete security sandbox. Native decoders, inference backends, and plugins need explicit authority/resource contracts; the specific OS mechanisms and their supported-platform limits are not yet selected.

An in-process Rust trait bound such as `Send + Sync` does not restrict filesystem, network, or device access. The present plugin host is not evidence that the selected sandbox architecture exists.

Sandboxing also does not prove semantic correctness. A driver can misencode an otherwise approved motion command. Driver qualification and device-I/O admission are distinct obligations. Qualification must bind to the accepted driver/version; an update must not silently inherit old evidence. Who accepts qualification, what evidence is required, and how qualification is invalidated remain open.

The engine's verified guarantees must state assumptions about native runtimes, extension behavior, protocol conformance, and hardware. Untrusted input/output validation and a qualification gate do not automatically prove arbitrary third-party implementations correct.

### 8.3 Naming and source provenance

Audit source/dependency origin and attribution before deciding what to retain or replace. A legacy symbol is not by itself evidence of copying, and a rename does not establish independent origin. Preserve licensing/attribution obligations; no license change is approved here.

Use responsibility-based names for generic interfaces and accurate implementation names for concrete adapters. `PulsarApp` was discussed as an example replacement for `FunGenApp`, not as a final naming scheme. Algorithm/model-family branding should correspond to actual implementation and qualified artifacts, not aspirational comments.

The scope, method, evidence standard, and compatibility implications of the naming/source-lineage pass remain to be designed.

## 9. Verification and correctness strategy

The confirmed assurance target is critical-path formal work plus layered testing, not a claim that tests formally verify an entire application.

| Technique | Intended obligation | Limit to record |
| --- | --- | --- |
| Type/architecture checks | Prevent invalid value mixing, invalid construction, and forbidden dependency directions. | Types do not establish model accuracy or validate arbitrary external bytes without parsing/checks. |
| Known-answer tests | Check fixed numerical/protocol/transform cases against independent expected results. | Specify precision, tolerances and backend applicability; universal bitwise GPU equality is not assumed. |
| Property and metamorphic tests | Exercise invariants and controlled transformations for geometry, timelines, motion, and composition. | Sampling is not exhaustive proof. |
| Fuzzing | Search parsers, protocols, command interfaces and state transitions for counterexamples and resource failures. | Declare corpus, bounds, budgets, isolation and reproduction rules. |
| Mutation testing | Challenge whether tests detect meaningful behavioral changes. | Review surviving/equivalent mutations; a score alone is not correctness evidence. |
| Formal models/proofs | Authorization, revision transactions, job/device lifecycles and selected key invariants. | Declare assumptions/bounds and check correspondence to implementation; a disconnected model is not proof of running code. |
| Differential and integration tests | Compare independent implementations and exercise real caller/adapter compositions. | Shared bugs or invalid references can invalidate the comparison. |
| Real-media and hardware qualification | Evaluate actual perceptual quality, temporal fidelity, resource/performance behavior and device response. | Do not substitute synthetic tests or a build success for qualified results. |

Tests should exercise the same meaningful interfaces used by callers, with internal seams where needed for numerical kernels and fault injection. Provide controlled substitutes for clocks, I/O, runtimes, and devices rather than accidentally actuating hardware during routine tests.

Concrete tools, formal proof obligations, arithmetic/float treatment, trusted implementation surface, accepted bounds, CI budgets, mutation thresholds, fuzz duration, and release gate policy remain open. No such tests or proofs were run as part of writing this document.

## Resolved architecture agenda

The former ARCH-O01 through ARCH-O14 questions are resolved by the submitted decisions and selected contracts below. This replaces the open-question list; it does not claim implementation or qualification.

| Former item | Resolution and implementation home |
| --- | --- |
| ARCH-O01 | Four library boundaries, dependency rules and modern names: CONTRACT-01; incremental M1/M2 migration. |
| ARCH-O02 | Per-user engine lifecycle, scheduling and aggregate resource admission: CONTRACT-05; M2/M3/M5. |
| ARCH-O03 | Versioned local command/query/event API, pairing, errors and resynchronization: CONTRACT-06; M2. |
| ARCH-O04 | Distinct identities, rational source time, integer project time and labelled coordinate spaces: CONTRACT-02; M1/M2/M4. |
| ARCH-O05 | Durable engine-owned SQLite transactions, shared undo and explicit rebase: CONTRACT-04; M2. |
| ARCH-O06 | Immutable job inputs, distinct attempts, validated checkpoints and bounded retry: CONTRACT-05; M2/M3. |
| ARCH-O07 | Frame/transform/seek-matched observations and independent previews: CONTRACT-03; M1/M2. |
| ARCH-O08 | Checked neutral motion and explicit observed/predicted/inferred/synthesized/unavailable semantics: CONTRACT-02 and boundary catalogue; M1/M3/M4. |
| ARCH-O09 | Compact lineage, scoped references, pinning and deliberate export stripping: CONTRACT-07; M2-M6. |
| ARCH-O10 | Wasmtime plugins, native worker isolation, exact-build exceptions and assistant approval policy: CONTRACT-08; M2/M3/M5/M6. |
| ARCH-O11 | Profile adaptation, admission, human-accepted qualification and separately approved headless stopping bounds: CONTRACT-09; M5. |
| ARCH-O12 | Evidence-led lineage audit and public compatibility window: CONTRACT-11; M1/M2/M6. |
| ARCH-O13 | Kani/TLC obligations, correspondence tests and layered campaigns before Preview: CONTRACT-10; M1-M8. |
| ARCH-O14 | Architecture-first execution under ARCH-041, preserving M1-M8 acceptance scope: CONTRACT-11, the major roadmap and the architecture-first execution plan. |

## 11. When architecture and planning are done

Architecture discussion is complete only when module ownership, dependency direction, public interfaces, state/effect ownership, types/units/time/provenance, failure/recovery, permissions, plugin trust, and verification obligations are settled sufficiently to implement without inventing material decisions.

Implementation planning is complete only when that architecture has executable, ordered work packages with concrete acceptance scenarios, migration/compatibility handling, required evidence and explicit prerequisites. Known product decisions must remain traceable. Open items may be deferred only through an explicit scoped decision with an owner/resolution point, not by silently assuming a default.

At that point, explicitly tell the user: **Architecture and implementation planning are complete for the agreed scope.** State any intentionally deferred scope alongside that declaration. Until then, say what remains and continue design.

That declaration does not mean implementation, the bug pass, M1, or release qualification is complete. Those require their own evidence and authorization. Do not claim completion because documents exist, because a long interview has occurred, or because tests pass on only a subset of behavior.

## Architecture closure: 2026-09-11

**Architecture discussion is complete across all 11 areas. Implementation and qualification remain unfinished.** ARCH-001 through ARCH-041 record submitted user decisions. The technical choices below are selected engineering contracts approved through the architecture-closure plan, not quotations attributed to earlier user answers and not descriptions of already implemented APIs. ARCH-041 subsequently changes delivery order to architecture first, then functional quality/research, then hardening/qualification; the [architecture-first execution plan](PULSAR_ARCHITECTURE_FIRST_EXECUTION_PLAN.md) defines its milestones.

The [Pulsar Software Specification](PULSAR_SOFTWARE_SPECIFICATION.md), including its decision sources, remains the product authority. This record refines architecture without reducing required modalities, quality, performance, offline availability, compatibility or platform/vendor scope. The [Pulsar Major Roadmap](PULSAR_MAJOR_ROADMAP.md) owns staged delivery and evidence gates. Earlier source maps describe their inspected baseline only; this consolidation does not refresh upstream or establish that defects still reproduce unchanged.

### Precedence and settled qualifications

- The final contracts below supersede earlier proposals and statements that these interfaces remain open. Earlier submitted answers and historical source findings remain preserved.
- ARCH-041 supersedes earlier M1-before-architecture sequencing, not M1 correctness obligations or M2-M8 scope. Phase A must establish real composed paths and enforced ownership, not empty scaffolding. Structural integration, functional demonstration and qualification remain separate states.
- ARCH-024 and ARCH-025 qualify the sandbox default: an explicit human-approved, exact-build native exception enters the trusted computing base. It is not made safe by a scoped API grant. ARCH-040 separately requires per-invocation approval for assistant calls by default, with an explicitly risky, human-controlled standing-grant alternative.
- ARCH-026 and ARCH-027 require qualified device-side failure-stop behavior and explicit approval of its per-configuration bound before headless playback. No universal numeric stopping deadline was chosen. Attended-only status does not certify an unsupported mode safe.
- ARCH-018 selects durable commit acknowledgement; bundled SQLite is the selected implementation technology. ARCH-019 selects shared actor-labelled undo; undo and redo are new authorized transactions, not restoration of an old revision identifier.
- ARCH-021 permits snapshot jobs to finish after edits, not stale commits. ARCH-022 permits eligible resume when the engine next starts, not OS-login autostart or device rearming.
- ARCH-028 permits one neutral program with up to six axes and multiple contributing sources/entities. It does not authorize simultaneous multi-device playback.
- ARCH-034 permits prominently labelled unqualified custom generation in normal projects, not silent degradation of qualified tiers or bypass of plugin, driver or device authority.
- ARCH-035 permits approved official qualification records or explicit human acceptance after mandatory local qualification. Running checks and accepting their result are separate authorities.

### CONTRACT-01: Modules, ownership and modern names

Use four Rust libraries and one packaged executable. Library boundaries express dependency and authority rules; they do not require a separate process for every logical module.

| Package | Owns | Forbidden responsibilities |
| --- | --- | --- |
| `pulsar-core` | Checked domain types, pure media/geometry/motion kernels, explicit state transitions and invariant predicates. | Unsafe code, I/O, GUI/runtime dependencies, database access, grants or device effects. |
| `pulsar-protocol` | Versioned wire DTOs, framing/schema validation, typed error/event shapes and checked conversion to domain values. | Project mutation, persistence, scheduling or effect authority. |
| `pulsar-engine` | Authorization, transactional projects, scheduler, job/attempt state, artifact/provenance stores, effect brokers and device sessions. | Presentation state or unmediated extension execution in its address space. |
| `pulsar-clients` | CLI and GUI adapters, presentation, transient editing gestures and independent previews. | Authoritative project writes, direct device access or a second generation implementation. |
| `pulsar` executable | Composition root and dispatch to client, per-user engine or isolated worker roles. | Business rules duplicated between process entry points. |

Dependency arrows mean "may depend on":

```text
pulsar executable
  -> pulsar-clients -> pulsar-protocol -> pulsar-core
  -> pulsar-engine -> pulsar-protocol -> pulsar-core
  -> worker-role composition -> protocol/core + isolated effect adapters

GUI / CLI / third-party client -- secured local API --> per-user engine
engine -- bounded worker protocol --> isolated decoder/inference/plugin/driver worker
worker -- immutable observations or candidate artifacts --> engine
engine -- checked durable transaction --> project store
```

There is no client-to-engine implementation dependency and no core-to-protocol, client or effect dependency. Pure kernels receive validated values and return explicit results. Unsafe/FFI code is confined to reviewed adapters outside core. Native runtime dependencies are role/feature-scoped so launching a basic client or engine does not initialize every backend.

Logical modules are concrete responsibilities inside these packages, not additional mandatory libraries:

| Logical module | Core responsibility | Engine/effect responsibility | Client responsibility |
| --- | --- | --- | --- |
| Media input/parser | Identity, metadata contracts, time/space mappings and buffer validation. | Import, snapshot, decode/probe workers and artifact access. | Select/relink sources and show explicit availability. |
| Media viewer | Frame/evidence association and transform composition. | Cache lookup and lightweight preview-analysis jobs. | Decode/display presentation, viewport and independent playhead. |
| Media editor | Edit algebra, protected regions and motion invariants. | Authorized revision transactions, candidate rebase, undo/redo. | Gesture previews and commit requests; not a full video NLE. |
| General DSP and optical flow | Pure filters, resampling, geometry, Lucas-Kanade kernels and track state transitions where practical. | Budgeted execution, artifact handling and worker lifecycle. | Parameter controls and truthful diagnostics. |
| Neural pipeline | Tensor/pre/postprocessing contracts, fusion semantics and checked outputs. | Model catalogue, local runtime workers, scheduling and qualified execution profiles. | Tier selection and qualified/unqualified status. |
| Local assistant | Typed action proposals and domain validation shared with other clients. | Capability broker, approvals, resource accounting and audited command execution. | Conversation and explicit approval surfaces. |
| Hardware doctor | Evidence/configuration types and eligibility predicates. | Read-only probes by default; separately authorized physical qualification and admission. | Explain unsupported configurations, required evidence and human acceptance. |
| Assurance suite | Harnesses for kernels and invariants. | Fault, protocol, lifecycle, backend and hardware harnesses. | GUI/CLI parity and real observation-rendering scenarios. |

Rename `FunGenApp` to `PulsarDesktop`. Preserve attribution and perform the agreed source/model lineage audit before redistribution claims. A rename, rewrite or new crate layout does not establish clean-room provenance.

### CONTRACT-02: Domain identities, units and evidence

| Term | Meaning and invariant |
| --- | --- |
| Project / `ProjectId` | One independent project time domain, one master motion timeline and one revision history. |
| Source version / `SourceVersionId` | Content-bound identity of immutable source bytes, distinct from a mutable filesystem path. |
| Source placement / `SourcePlacementId` | A version's explicit mapping into project time; the same source may have different placements. |
| Frame / `FrameId` | A decoded sample identified within a source version with its source timestamp/sample identity; not a GUI frame counter. |
| Entity track / `EntityTrackId` | Perception identity with explicit lifecycle, continuity and cut/reset scope. |
| Motion target / `MotionTargetId` | Selected entity, relationship, region or synthetic intent from which motion is derived; not a hardware identity. |
| Motion output / `MotionOutputId` | The project's neutral program, with up to six named axes in the first release. |
| Revision / `RevisionId` | A committed project state. Undo/redo creates a new revision rather than reusing one. |
| Candidate / `CandidateId` | Proposed motion/edits bound to an input revision and lineage; not committed state. |
| Job / `JobId` | Stable requested work against an immutable input manifest. |
| Attempt / `AttemptId` | One execution with exact model/runtime/backend/configuration dependencies. |
| Artifact / `ArtifactId` | Immutable, validated content with an explicit owner, format, size and retention policy. |
| Physical device / `PhysicalDeviceId` | A hardware endpoint identity, distinct from an output, profile, driver or transport connection. |
| Device session / `DeviceSessionId` | Engine-owned controller authority, admission and playback epoch for a device. |
| Grant / client session | Explicit operation/project authority and authenticated connection context; neither is an arbitrary process credential. |
| Provenance reference / `ProvenanceRef` | Opaque reference to compact lineage, not a copied graph or implicit read capability. |
| Qualification / admission | Qualification binds evidence to an exact configuration; admission checks a particular operation/program against it and current authority. |

Preserve rational source PTS/timebase, checked signed integer-nanosecond project time, and a separate monotonic playback deadline type. Source placement mappings are explicit rational transforms, including offsets and rate changes. Conversions check denominators, range and overflow. Floating-point frame indices must not replace source timestamps. Quantization occurs only at declared export/transport boundaries with a recorded rounding/error report.

Changing source alignment leaves existing motion unchanged and marks affected derived results stale. Retiming/regeneration is an explicit edit. Seeks, cuts, discontinuities and source replacements reset the relevant association/tracker epochs rather than pretending continuity.

Neutral positions are finite checked values in `[0,1]`, with explicit axis identity, direction conventions and availability. Missing axes are not silently zero-filled. Relative motion is not automatically stretched to full range; physical distance/angle conversion belongs to a device profile. Detailed six-axis encodings and profile tolerances must be published and qualified in M4/M5 without changing these invariants.

Coordinates always identify their space: decoded source, model tensor, crop, rotated image, projection or viewport. Transform composition carries version/identity and dimensions. Invalid, non-finite, reversed or degenerate geometry produces typed invalid/unavailable results; clipping and normalization are explicit operations, not a reason to fabricate a valid detection.

Every span or observation distinguishes observed, tracked prediction, inferred, synthesized and unavailable evidence. Prediction carries age and its last observation association. Confidence includes whether it is calibrated; an uncalibrated score is not a promised probability. Text/still-image synthesis and preset motion are labelled synthesis, never detector observations.

### CONTRACT-03: Viewer and tracking

The viewer reuses matching observations or requests lightweight on-demand analysis when none exist. Rendering does not depend on completing a generation job.

An observation may render only when source version, displayed frame, source placement/mapping, transform context and seek/request generation match. A late result can enter a correctly keyed cache but cannot overwrite current display state. On a seek, source change or incompatible transform, hide stale boxes while awaiting matching evidence.

Actual detections, bounded-age track predictions and user-defined analysis regions have distinct labels/styles. A fixed user region is legitimate only as a labelled region; it cannot stand in for tracking. Missing detections show unavailable/no-observation state rather than a decorative box. Resizing/projection changes must recompute or invalidate the applicable display transform.

Client previews have independent playheads. Attaching to physical playback displays the engine session's admitted revision and clock explicitly; preview seeks or edits do not move the device.

### CONTRACT-04: Projects and persistence

Use bundled SQLite: no user-installed database service or library. Bundled compilation is a build-time concern, not an end-user installation step. [Bundled SQLite build documentation](https://github.com/rusqlite/rusqlite#notes-on-building-rusqlite-and-libsqlite3-sys).

Only the engine commits. A commit validates the authenticated actor/grant, project scope, protected-region permissions, expected revision, request identity and domain invariants. Revision state, compact lineage, request outcome and the corresponding durable event are recorded atomically. Acknowledge success only after the transaction's configured durable synchronization completes. Hardware/filesystem durability assumptions remain explicit; transient gesture previews are not acknowledged commits.

Interactive gestures form atomic edits. Shared actor-labelled undo targets the latest committed project edit regardless of originating client. Undo/redo creates a fresh revision and rechecks current permissions/protection. Conflicting or protected changes are not bypassed by history operations.

Candidates retain their input revision. Stale candidates require explicit rebase and then a fresh expected-revision commit; rebase does not grant commit authority or silently replace user edits. Rebase failures remain inspectable candidates.

Generation media is an immutable engine-managed snapshot, using copy-on-write where supported and otherwise copying. Content identity, preparation time and required disk space are explicit. Linked mutable originals are not a silent substitute when snapshot admission fails.

Recovery restores committed state and eligible job records. Migrations preserve a recoverable original and do not overwrite the sole project copy before successful migration. Portable exports capture a consistent database/artifact snapshot, not a raw copy of live database files. Missing media can be relinked only to matching content for exact replay; changed content is a new source version.

### CONTRACT-05: Engine, jobs and resources

One engine per OS user owns project writes, resource scheduling and device sessions. Secured discovery and instance exclusion prevent accidental duplicate owners. Incompatible clients must report protocol incompatibility, not kill or replace a running engine. Closing a GUI does not stop authorized offline jobs; this does not authorize OS-login autostart.

A job has immutable input/configuration identities and separate attempts. Every attempt records exact runtime/backend/model/transform dependencies and produces immutable artifacts or a candidate, never a direct project mutation. Edits during generation do not cancel it automatically; completed old-revision work remains stale until explicit review/rebase.

Eligible interrupted jobs resume when the engine next starts only from validated checkpoints and matching dependencies. Checkpoint publication is atomic and includes progress, content identities and compatibility metadata. Missing/evicted prerequisites block recovery unless restored from matching content.

Automatic backend failure may start a new attempt on a qualified equivalent preserving the requested quality tier. Disclose backend and performance changes. Explicit backend pins are strict. Never reuse incompatible checkpoints or silently downgrade into a lower tier or unqualified custom run. Retries and crash recovery loops are bounded.

Protect control/device/live deadlines first, then interactive preview/assistant work, then offline generation with project fairness. Assistant requests may suspend offline work for resources, not disrupt live control. Account for engine, workers, model residency, plugin memory, buffers, checkpoints and snapshots in aggregate CPU/RAM/VRAM/storage budgets.

Insufficient resources block admission or expose a recoverable blocked state; do not silently weaken durability, isolation, timing or output guarantees. Pinned or authoritative data cannot be reclaimed to make admission succeed. Performance gates remain end-to-end and include required preparation, not inference-only timing.

### CONTRACT-06: Local API and authority

Use versioned, length-framed UTF-8 JSON over secured per-user Unix sockets or Windows named pipes. Protocol v1 bounds control messages at 1 MiB. Bulk frames/artifacts use separately bounded authorized streams or immutable read-only buffer handles. The transport library supplies local sockets, not the application framing or authority policy. [Local IPC documentation](https://docs.rs/interprocess/latest/interprocess/local_socket/index.html).

Endpoint naming alone is not authentication. Use platform-appropriate per-user endpoint ownership/access controls, peer/session validation and pairing. Do not treat a generic namespace or shared temporary directory as equivalent security across platforms. [Endpoint naming documentation](https://docs.rs/interprocess/latest/interprocess/local_socket/enum.GenericNamespaced.html).

The published request envelope identifies protocol version, request ID, authenticated client session, operation, validated arguments, project scope when applicable and expected revision for project mutation. Request identifiers are scoped to authenticated identity; reusing an identifier with different arguments is an error.

| Surface | Contract families | Required semantics |
| --- | --- | --- |
| Project commands | Create/open project, import/relink source, set alignment, apply edit, undo/redo. | Engine authorization, checked domain conversion and atomic expected-revision mutation. |
| Job commands | Start/cancel/resume job, rebase/commit candidate. | Immutable manifests, distinct attempts, cancellation fencing and explicit stale handling. |
| Artifact commands | Export/package, pin/unpin, authorized artifact transfer. | Consistent snapshots, bounded transfer, deliberate metadata stripping and local receipts. |
| Authority commands | Pair, grant/revoke, controller acquire/release. | Explicit human-owned elevation, scoped grants and revocation effects. |
| Device commands | Arm, start/stop and switch playback revision. | Qualified admission, controller ownership, revision pinning and explicit session epochs. |
| Queries | Project/job/candidate snapshots, capabilities, qualification and session state. | Coherent revision/identity tags; no mutation hidden inside reads. |
| Durable events | Revision committed, grant revoked and authoritative job/session transitions. | Ordered cursor within its documented scope; reconnect replay or snapshot resynchronization. |
| Ephemeral events | Progress, preview frames and transient diagnostics. | Bounded queues; coalesce/drop under backpressure, never counterfeit durable outcomes. |

Publish command/query/event schemas, field constraints, compatibility negotiation and typed errors. Error families include invalid input, unauthorized/revoked grant, revision conflict, protected edit, missing dependency, resource blocked, incompatible checkpoint/protocol, unqualified configuration, cancelled work and unknown external-effect outcome. Unsupported operations and malformed data fail explicitly, not by panic or implicit fallback.

For committed project mutations, persist request identity and outcome in the same transaction as the edit. Retrying the same request returns its recorded outcome rather than creating a second edit. For external/native/device effects, distinguish accepted, dispatched, acknowledged and unknown outcomes; do not claim exactly-once physical effects or blindly replay an ambiguous command.

Reconnect provides a snapshot plus a valid event cursor; expired/gapped cursors require resynchronization. Authority and current revision are rechecked after reconnect. Client disconnection is not grant revocation. Revocation blocks new work, cancels work authorized solely by the revoked grant and withdraws associated device/controller authority; late results cannot restore it.

No arbitrary shell, SQL, raw project-memory mutation or ambient device access is exposed. Third-party clients pair explicitly and receive project/operation grants; device and external effects remain separately scoped. This is not protection against a fully compromised same-user OS account.

Buffers validate identity, size, dimensions, strides, element type and ownership before allocation/use. Shared handles are immutable, authorization-scoped and lifetime-managed. Never reuse a client-visible shared object for future unauthorized data. Revocation can deny new transfers but cannot make already disclosed bytes unread.

Public wire compatibility does not promise a stable internal Rust ABI. GUI and CLI use the same engine contracts; the GUI is not a background terminal wrapper.

### CONTRACT-07: Provenance, privacy and disposal

Compact lineage links source versions and observations to model/runtime/algorithm/configuration identities, transforms, candidates and committed output spans. Hot paths carry opaque references; consumers resolve only the lineage they are authorized to access. Provenance is not an authority token.

Keep active job/checkpoint/candidate dependencies pinned until their documented consumers release them. Explicit user pins and packaged projects retain required media for self-contained replay. Once unused and unpinned, source snapshots and other recomputable cache artifacts are evictable. Retain committed motion, compact lineage and source content identities; original media and authoritative project state are not cache-eviction targets. Eviction may make an exact rerun unavailable until matching content is restored, which must be shown honestly.

Heavy traces are opt-in, bounded and separately disposable. Avoid credentials, prompt/media contents or source-identifying metadata in ordinary logs. Retention and diagnostic export permissions are explicit.

Standard funscript exports and device payloads strip internal/source-identifying provenance at that boundary while preserving local adaptation/export receipts. Explicit portable project packaging is a different operation with a declared manifest, not accidental inclusion of private lineage in standard motion files.

Official engine-mediated processing remains local/offline as specified. Authorized online device/service operations remain explicit effects. User-approved unsandboxed native code is outside confinement/privacy enforcement guarantees and must be labelled accordingly.

### CONTRACT-08: Plugins, models and runtimes

Use Wasmtime/WASM for bounded processing/editing extensions. Native runtimes, codecs and drivers use isolated worker interfaces. Do not load third-party extension code into engine or GUI address space.

Role-specific manifests specify artifact identity, interface version, required capabilities, buffer/tensor formats, preprocessing/postprocessing, budgets and effect requests. Validate tensor rank/shape/type/size, image transforms and output values before use. Model import does not implicitly execute arbitrary custom code or authorize downloads.

Host effects are brokered. Guests and workers cannot grant permissions, mutate project memory, self-qualify drivers or bypass device admission. Guest memory/compute limits do not bound blocking host calls: each broker call needs separate deadlines, cancellation and resource accounting. [Wasmtime resource-control limitations](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html#method.epoch_interruption).

Native confinement must be qualified on each supported platform. If unavailable, refuse execution unless the human approves the exact-build trusted exception. Changed bytes invalidate approval. The exception belongs to the trusted computing base and cannot be presented as sandboxed. Wasmtime platform support is a feasibility reference, not qualification of Pulsar's packaged configurations. [Wasmtime platform support](https://docs.wasmtime.dev/stability-platform-support.html).

For assistant invocation of unsandboxed extensions, exact-build load approval and invocation approval are distinct. Default: approve each proposed call. The human may separately enable an explicitly risky standing grant bound to exact build and declared call scope. The assistant cannot enable either the native exception or standing grant. Revocation or changed bytes invalidates authority; native effects remain outside sandbox guarantees.

The model zoo distinguishes qualified built-in execution profiles from structurally validated, prominently unqualified custom runs permitted in ordinary projects. Custom runs do not inherit official quality/speed claims or relax driver/confinement requirements. Qualified automatic selection cannot silently become a custom run.

Official core/complete packages bundle their required offline models/runtimes as verified, versioned resources with atomic first-use extraction and recoverable update/rollback. Large model archives are packaged resources, not enormous linked program-data sections. No silent system-package installation or network bootstrap is required for the promised offline core. Preserve the specification's platform/architecture/vendor scope and release staging.

### CONTRACT-09: Device admission and qualification

Keep neutral motion, profile adaptation, admission, driver encoding and transport separate:

```text
neutral program + revision
  -> profile adaptation + local receipt
  -> engine admission against grants, limits and qualification
  -> qualified driver encoding
  -> transport / physical device
```

Adaptation preserves event timing and constrains amplitude to the selected device profile, with explicit physical-unit conversion. It cannot invent full-range motion or silently retime to pass limits. If constraints cannot be met under this policy, block admission or require an explicitly reviewed alternative rather than weakening limits.

Each physical device has exclusive engine ownership and one authorized session controller. Playback is pinned to its admitted motion revision. Ordinary edits and independent preview seeks do not change physical commands. A controller-authorized revision switch requires re-admission and stop/rearm where the transport cannot switch safely. Live-input generation is a distinct authorized session mode.

Normal playback requires an approved official qualification record or explicit human acceptance after required local qualification. Evidence/acceptance binds exact driver artifact, physical device, firmware and transport configuration; changed configuration invalidates applicability. The assistant may help execute authorized checks but cannot supply human acceptance. Generation/model qualification does not substitute for driver qualification.

Controller loss stops by default unless explicit headless mode is granted. Headless additionally requires a qualified device-side stopping bound after engine/connection failure and explicit approval of that bound for the actual configuration. A sent or acknowledged stop command does not prove physical stopping. Unknown physical state remains unknown.

Restart/reconnect remains disarmed; old commands/session epochs cannot rearm implicitly. Separate physical test mode requires explicit authorization and cannot promote itself to normal playback or qualify its own acceptance.

### CONTRACT-10: Assurance and evidence

Use Kani for bounded critical Rust kernels and TLA+/TLC for lifecycle models, with separate implementation-correspondence tests. Record assumptions, bounded domains, unwinding limits, numerical tolerances and trusted components. Kani does not prove the whole concurrent application; TLC checks a specified model, not arbitrary Rust implementation. [Kani documentation](https://model-checking.github.io/kani/print.html), [TLC documentation](https://docs.tlapl.us/using:tlc:start).

| Obligation | Owner / boundary | Planned assurance | Required delivery gate |
| --- | --- | --- | --- |
| No cross-project or operation authority escalation | Engine broker and protocol conversion. | Bounded grant predicates; lifecycle model; negative integration tests and protocol fuzzing. | M2 implementation evidence; critical formal obligations before M7 Preview. |
| No partial or stale durable commit | Core revision transition and engine transaction. | Kani checked predicates; TLC commit/conflict model; crash injection and durable-outcome retry tests. | M2; M7 release-bound proof/correspondence evidence. |
| No late result accepted under cancelled/revoked authority | Job/attempt and grant lifecycle. | TLC race/interleaving model; worker fault injection; correspondence tests. | M2/M3; M7. |
| Exclusive device ownership and no stale session replay | Device controller, admission and session epoch. | Lifecycle checking, protocol tests, reconnect fault injection and real transport qualification. | M5; M7, with physical evidence separately required. |
| Checked time/space/tensor/buffer arithmetic | Core types and trust-boundary adapters. | Kani bounded arithmetic; KAT; property/metamorphic/differential tests; malformed-input fuzzing. | M1-M4 affected surfaces; M7 critical formal gates. |
| Truthful frame/evidence association | Perception-to-viewer boundary. | KAT/property tests, seek/cut/resize/late-result scenarios and real detector media. | M1/M2; no synthetic-only detector qualification. |
| No invented stroke or timing distortion | Motion/DSP/adaptation boundaries. | Independent KATs, metamorphic properties, differential checks and annotated media/device comparisons. | M1/M3/M4/M5 and existing quality gates. |
| Enforced confinement and bounded resources | Runtime/plugin/buffer brokers. | Malicious guest/host-call fixtures, deadline/OOM/storage/crash injection and platform qualification. | M3/M5/M6; M7. |
| Correct public behavior across clients and migrations | CLI/GUI/protocol/store adapters. | Contract parity, non-destructive migration, reconnect and portable-export scenarios. | M2/M6; M7/M8. |

Use independent known answers, property/metamorphic/differential tests, fuzzing, mutation testing, fault injection, real-media benchmarks and hardware qualification. Mutation campaigns must kill critical invariant mutants or record reviewed equivalence evidence; tests derived solely from the implementation are not an independent oracle.

Fast PR checks cover contracts, KATs, properties and targeted critical proofs. Deeper affected-surface and release campaigns cover fuzzing, mutation, native backends, performance and physical hardware. Every receipt binds its actual source/build/artifact/configuration, tool version, assumptions and result. Missing, unsupported, stale or failed evidence is not PASS.

Defined critical formal obligations gate the first public Preview. Neural accuracy, foreign runtime correctness, operating-system guarantees and physical safety are not established by these proofs. Development/CI proof tools are not end-user runtime dependencies. Existing product quality, performance and physical qualification gates remain mandatory.

### CONTRACT-11: Migration, compatibility and milestones

Preserve documented CLI syntax/aliases and non-destructive project imports through the first Stable major series. Remove deprecated public interfaces only in a later major with a migration path. Preserve standard funscript interoperability permanently, not historical defects.

Replace duplicated GUI/CLI/batch generation incrementally through shared contracts. Every transitional adapter must record its owner, supported compatibility surface, replacement gate and removal milestone. Modernize internal names without rewriting provenance or licensing history.

ARCH-041 selects architecture-first delivery: Phase A establishes the final four-library ownership/dependency shape and real integrated interface paths; Phase B performs functional implementation, deep technical research and correctness closure; Phase C completes hardening and qualification. The [architecture-first execution plan](PULSAR_ARCHITECTURE_FIRST_EXECUTION_PLAN.md) owns the A1-A6, B1-B6 and C1-C3 execution milestones.

M1 retains its baseline, bounding-box, tracking and stroke-correctness acceptance scope, but no longer precedes architecture as a mandatory execution stage. Re-establish the selected current baseline without overwriting local documentation or unrelated work. Reproduce the historical 20 regression cases against the migrated/current implementation; do not assume old failures or apply cached patches blindly.

Phase A must retain known defects explicitly, correct migration-created regressions and minimum data-loss/authority/unsafe-effect blockers, or disable affected paths. Its real vertical slice must use GUI/CLI engine interfaces, immutable worker input, a revision-bound candidate, authorized durable commit and neutral export. Empty modules and success-returning stubs do not satisfy architectural completion.

Phase B must fix demonstrated detector shape/coordinate validation and clipping, tracker identity/aging/cut behavior, box propagation, forward/backward-flow defects and stroke artifacts. Preserve pauses, shallow motion, reversals and plateaus; eliminate invented full-range movement. Connect actual observations to GUI rendering and exercise moving targets, absent detections, seeks, cuts, resizing, projection changes and late results. Real-media behavior is separate from synthetic/unit evidence; a moving synthetic box does not qualify a real detector.

Deep research follows stable architecture and measurement interfaces. Record falsifiable hypotheses, independent oracles, exact artifacts, budgets, results and negative findings. ADRs record decisions and link reproducible experiment evidence; an accepted ADR is not qualification. Fix deterministic bugs that invalidate an experiment before trusting its measurements.

The M1-M8 table below preserves delivery scope and acceptance obligations; it is not the chronological execution order. No product requirement or critical formal/device gate is weakened by the new sequence.

| Milestone | Contract delivery without scope reduction |
| --- | --- |
| M1 | Reproduced baseline defects, truthful box/flow/frame boundaries and stroke correctness, with separate real-media evidence. |
| M2 | Shared engine/API, checked domain contracts, durable projects, revocation/revision lifecycle and GUI/CLI/batch parity. |
| M3 | Qualified local inference, explicit attempts/fallback, bounded resources, assistant authority and reference performance. |
| M4 | All required modalities, projections, labelled synthesis and six-axis neutral output. |
| M5 | Live deadlines, profile admission, Handy/Handy 2 modes, controller/session lifecycle and hardware qualification. |
| M6 | Portable offline packages, extraction/migrations, compatibility and required OS/architecture/GPU-vendor qualification. |
| M7 | Preview product gates plus required critical proofs, models, correspondence and candidate-bound evidence. |
| M8 | Stable and comparative qualification, preserving all specification quality/performance/device obligations. |

### Boundary contract catalogue

These are target interface families, not claims that callable interfaces or wire schemas already exist. Each implementation must publish its checked input/output types, error cases, ownership, resource lifetime and evidence obligations before replacing a legacy seam.

| Boundary | Input / output | Authority and invariant |
| --- | --- | --- |
| Import / decode | Source request -> immutable source version, metadata and source-timed frames. | Engine owns access and snapshots; malformed media is untrusted; buffer allocation is bounded. |
| Inference | Frame/tensor plus artifact/configuration identities -> validated observations. | Isolated runtime, explicit model/source transforms and availability; no project/device mutation. |
| Tracking / flow | Ordered frame/evidence state plus cut/reset context -> tracks/predictions and next state. | Explicit identity/age, dimension compatibility and backward-check semantics; no invented observation. |
| Motion derivation / DSP | Target-bound evidence or labelled synthesis -> neutral motion candidate. | Checked units/timing, uncertainty/provenance and amplitude preservation. |
| Project edit | Actor/grant, request identity, expected revision and edit/candidate -> new revision or typed failure. | Engine-only atomic durable transaction; stale/protected changes cannot bypass checks. |
| Worker execution | Immutable job/attempt manifest -> checkpoints, progress and immutable artifacts. | Worker cannot commit or regain revoked authority; engine owns accepted transitions. |
| Preview | Displayed-frame/transform/request identity -> matching observations or explicit unavailable state. | Independent client playhead; stale evidence cannot render as current tracking. |
| Device adaptation / admission | Neutral revision plus profile/configuration/authority -> admitted session or rejection. | Preserve timing, constrain amplitude, bind exact qualification and controller. |
| Driver / transport | Admitted commands plus session epoch -> encoded payload and explicit outcome. | No ambient access, no implicit rearm, unknown outcome is not physical completion. |
| Doctor / assistant | Diagnostic/action proposal -> bounded evidence or authorized engine command. | Human-only acceptance/elevation remains outside tool authority. |

### Lifecycle invariants and trust boundaries

- Acknowledged project edits survive supported crash recovery under documented storage assumptions; cancelled previews are not commits.
- A job's input revision and content manifest never change in place. Backend changes create attempts; stale results cannot silently become current edits.
- Cancellation/revocation is fenced at result acceptance and commit, not merely signalled to a worker. Late work cannot recreate a grant.
- Normal client disconnection preserves authorized offline jobs; controller-loss device policy is a separate lifecycle rule.
- Preview association is keyed to source/frame/mapping/transform/request context. A matching rectangle shape alone is not evidence freshness.
- Device arming/admission, running, stop-requested, stopped/disarmed and unknown physical state remain distinct. Software acknowledgement cannot collapse those states into a physical-stop claim.
- Playback revisions and session epochs are explicit. Reconnect, crash recovery or project edits cannot replay authority or rearm.
- Parser, model, plugin, native worker and wire outputs are untrusted until validated. Checked core types do not make foreign code trusted.
- Per-user IPC pairing limits supported clients; it does not defend against a compromised same-user OS account. Native trusted exceptions weaken confinement explicitly, not invisibly.
- Source originals, committed motion and compact lineage are authoritative/user data, not cache victims. Disposal cannot invalidate active dependencies silently.

### Closure record and remaining execution

All 11 architecture areas and ARCH-001 through ARCH-041 are now recorded. No architecture preference question remains open. ARCH-041 revises execution order without reopening the settled module/trust contracts. Concrete implementation schemas, migration adapters, model/backend selection, dataset curation, numerical tolerance qualification, source/model lineage evidence, platform packaging and physical-device evidence remain delivery work under the architecture-first milestones and preserved M1-M8 acceptance scope.

This consolidation changes documentation only. It applies no production patches, runs no tests/proofs/benchmarks, refreshes no upstream baseline, qualifies no model/device/platform and creates no commit. Historical regression results are not fresh qualification of this document's target architecture.


Current status: **ARCHITECTURE DISCUSSION COMPLETE. Implementation, testing, proofs, benchmarks and hardware qualification remain unfinished.**

The user's requested completion signal is `pineapple`. Architecture closure satisfies the discussion milestone only; it is not a claim of implementation, release readiness or physical qualification. Subsequent changes to settled contracts must record their decision source and supersession explicitly.


## ARCH-042: explicit per-action source-clock selection

Date: 2026-10-09. Decision source: implementation under the owner's instruction to complete the architecture-first roadmap, grounded in the retained native mixed-clock counterexample. This adds a core arithmetic interface; it supersedes no ownership, trust, execution-order or publication-authorization contract in ARCH-001 through ARCH-041.

Mixed-clock composition uses explicit exact action-time/carrier selections rather than interval membership or value equality. Core validates arithmetic, admission, source-version consistency and action correspondence, and returns source-ordered mappings with parallel carrier indices. Engine remains responsible for verifying immutable ancestry and retaining versioned proofs before effects. Unknown selections, absent ancestry and source collisions remain refusals. The existing publication guard stays enabled until that integration is proved. See [selected clocks](PULSAR_SELECTED_SOURCE_CLOCKS.md) for the callable contract, tests, unresolved interpolation/resampling obligations and integration gate.


## ARCH-043: durable native action-origin timing proofs

Selected contract: native placed Stash timing proofs use schema 4. Their context retains a bounded carrier table containing each original candidate/job, original canonical motion and producer receipt identities, immutable placement and ordered retiming revisions. Each mapping has one explicit carrier selection in elapsed-source order, keyed by its current project timestamp. The proof remains bound to the source capture, publication request, current master revision/hash and exported script identity. It confers no new publication or device authority.

Native observation authenticates the same proof bytes, bounds carrier/selection/mapping arrays and rederives the complete action correspondence from retained native producer, edit, CAS and revision evidence. It requires exact agreement with the stored carrier facts, selections, mappings and reuse/action counts, and checks the source catalogue identity. Missing or altered evidence leaves the proof unverified. Schema 1/2 legacy readers remain supported for retained legacy proofs; schema 3 remains the distinct archived context. A schema 1/2 document with a schema-4 ancestry section refuses verification rather than silently downgrading that section. Readers without schema-4 support cannot classify its origin as verified.

This provides durable native action facts for supported source-time publication. Enabling mixed-clock publication still requires portable correspondence that preserves those facts, together with collision/trim/ordering and old-reader gates. The current selected-anchor comparison remains enabled until that work is verified. Qualification, physical device and human/release acceptance remain separate.


## ARCH-044: retained archival action-origin proof verification

Selected implementation contract, 2026-10-09: placed archival Stash publication retains schema 5, with the same bounded exact action selections as native schema 4 plus an explicit original manifest identity for each archival carrier. Imported revision, candidate, producer, edit and retiming correspondence must be verified through retained objects. Historical IDs are attribution only and never job execution or publication authority. Native schema-4 serialization remains unchanged when the archival namespace is absent.

Observation rederives archival action facts before accepting schema 5. Sampled schema-3 legacy mappings are rederived as well; an ancestry-free schema downgrade cannot bless a forged mapping. Unplaced schema 3 retains its separate unsampled convention. This extends ARCH-043's archival proof contract without superseding ownership, authority, execution order or qualification requirements. The existing selected-producer guard remains enabled until complete multi-carrier portability is verified. [Portable action-proof evidence](PULSAR_PORTABLE_ACTION_CLOCK_PROOFS.md) records the actual predecessor defect, current replay, unsuccessful complete campaigns and remaining gates.


## ARCH-045: native multi-carrier proof implementation candidate

Implementation candidate, runtime verification pending: native explicit source-time export uses each effective action's verified producer clock. The selected anchor identifies a contributing native producer rather than a universal clock. Its candidate/job/program/receipt/placement identities must match retained evidence. Multi-carrier native proofs use schema 6 with bounded carrier selections, individual ordered retiming chains and a sorted unique context catalogue of referenced retiming receipts. Observation rederives those facts and checks the anchor and catalogue. Single-carrier schema 4 retains its wire shape; native legacy schema 1/2 mappings also undergo derivation.

This candidate replaces the uniform-clock and selected-mapper comparison only on native explicit source-time export. It does not relax non-source-time, archival or native-archival mixture guards. It extends ARCH-042/043's arithmetic and retained-action contract; their runtime, portability, collision, old-reader and qualification obligations remain open. [Implementation and unsuccessful runtime attempts](PULSAR_NATIVE_MIXED_CLOCK_EXPORT.md) distinguish compilation from actual qualification. No complete native mixed-clock campaign or release readiness is asserted.

## ARCH-046: separate completed WASM operational visibility from invocation custody

Status: implemented and scoped engine/CLI/runtime checks verified; full qualification remains open.

The previous 64-row WASM admission limit counted completed invocations together with unfinished operations. Successful jobs could therefore exhaust future operational capacity, even though their immutable module/input/output bytes, receipts and candidate associations must remain available for provenance and portable packaging. Deleting completed rows would violate custody. Raising the operational bound alone would preserve that coupling.

Use explicit identified ArchiveCompletedWasmEdit against the observed attempt and original start request. The owning session needs current Read and Edit authority, and worker custody must have retired. An archive marker removes a completed row from the bounded operational catalog; it deletes no invocation bytes, receipt, lineage, candidate, master or revision. Status by original attempt and identified start/archive replay remain available. Portable format 12 continues to read all completed invocation custody independently of catalog visibility.

Keep operational admission at 64 and separately bound total retained invocation rows at 256. New work refuses without discarding evidence when either required capacity is exhausted. This is a bounded storage policy, not an unlimited-history claim; explicit durable-evidence disposal/export custody and broader quota qualification remain required.

Arm a conditional schema-26 reader barrier in the same transaction as the first archive marker and its remembered response. Preserve the underlying reader layers, including portable WASM schema 25. Old engines refuse schema 26; stripped/downgraded/future/malformed barriers must refuse before writable startup. Keep the main immutable invocation schema and portable package format unchanged. The CLI requires a supplied archive request ID for exact lost-response replay. Desktop archival presentation and durable explicit recovery are tracked in [desktop archival](PULSAR_DESKTOP_WASM_ARCHIVAL.md); [retained history browsing](PULSAR_WASM_ARCHIVE_HISTORY.md) now queries owning-session archives in bounded current-Read-authority pages without restoring execution/commit authority. Full process-death/power-loss/performance/platform qualification remains open.

Implementation and evidence are tracked in [completed invocation archival](PULSAR_WASM_COMPLETED_ARCHIVAL.md). The final test/runtime results must establish the scoped claim; the ADR alone is not qualification.


## ARCH-047: resolve unpublished WASM starts through the actor request namespace

Status: implemented; verification tracked in [start resolution](PULSAR_WASM_START_RESOLUTION.md). Full qualification remains open.

Catalog absence is an observation, not proof that an unknown start cannot execute later. Use explicit captured metadata and current Read/Edit authority to discover a matching accepted invocation, or reserve a free original ID in the engine's existing global actor request namespace. Publish that reservation and the identified resolution outcome atomically. Keep the reservation's digest domain separate from typed execution fingerprints. A start's transaction rechecks its identity after preflight, so withdrawal and acceptance have one durable order. Occupied unrelated identities and missing retained accepted rows remain conflicts, never invented absence proofs. Accepted work is not cancelled by resolution.

Use saved schema-1 metadata to derive a stable resolution ID for desktop retry. Preserve inert recovery and fresh approval rules; remove exact unknown local facts only after checked withdrawal. Accepted outcomes preserve attempt/artifact/master custody and use independent status/cancellation/review. Keep storage/portable formats unchanged and exercise the exact preceding executable's existing namespace fence. This extends recovery authority without granting execution, standing approval, direct project writes or external effects.


## ARCH-048: package a dedicated data-only worker executable

The GUI/CLI executable remains the per-user engine composition. A second binary-only workspace package, `pulsar-worker`, composes the existing engine worker roles without `pulsar-clients`, `egui`, or `eframe`. Both entrypoints share the engine's role dispatcher so bounded arguments, Linux-only roles and compiled feature refusals retain one implementation. Legacy worker roles on `pulsar` remain available for existing explicit configurations.

Owner startup selects an absolute `PULSAR_WORKER_EXECUTABLE` override or the companion beside the actual engine executable, with no PATH search or automatic GUI fallback. Explicit `EngineConfig::new` callers retain their chosen image. Missing/nonregular/empty images fail before durable engine-state creation; immutable snapshot capture and all later launch checks remain authoritative. Help/version selection does not require the companion to exist. Cargo default members build both binaries; installation respects Cargo's target directory and release archives include both. Optional features must be built consistently for both.

The existing WASM native-image ceiling remains 128 MiB. Editor availability and start admission now use that same ceiling before job publication, while executable capture still verifies regular bytes, execute mode and bounded size. A dedicated image is not a confinement proof, a native dependency closure, a firmware/device qualification, or a fix for the separately observed early filesystem synchronization delay.

Evidence and remaining gates are recorded in [dedicated worker composition](PULSAR_DEDICATED_WORKER.md).


## ARCH-049: bound inline protocol payload storage

Large control-message variants own boxed payloads while retaining their existing tagged JSON representation. Box only large payloads and update constructors and ownership boundaries in the engine, worker and clients; the client facade keeps its existing value types. This changes the Rust source interface without changing the protocol version or adding a wire wrapper.

Freeze preceding-source synthetic Generate, PackageDownload and bulk Ready encodings and check decode/re-encode compatibility. Bound inline RequestFrame, ResponseBody, WorkerOperation and WorkerOutput storage with regression tests. These limits describe enum storage, not heap usage, total memory or latency. Continue existing input, transport and resource limits independently.

See [protocol storage verification](PULSAR_PROTOCOL_STORAGE.md). Passing protocol lint and compatibility checks does not close the broader test, runtime, platform, media, device or release gates.


## ARCH-050: bound client reply and editor queue storage

Extend owned payload indirection to the client facade and private editor queues. Keep the hydrated ProjectSnapshot and CandidateSnapshot structures; their reply variants now own boxes. Forward existing compatible protocol boxes through the wire-to-client adapter, and unbox only when entering an existing owned-value controller API. Preserve tagged JSON and all current authority, revision, replay, hydration and transfer validation. This changes the public Rust client enum constructors, so bundled consumers and examples must compile against the new source interface. It does not change the engine protocol version.

Bound client reply inline storage to 256 bytes, private editor reply storage to 256 and editor action storage to 512. Keep a frozen package-download encoding check and existing headless/controller/native process regressions. These limits concern enum storage and move payloads to the heap; they are not total-memory, latency or hardware qualification claims.

Group file-generation inputs into one private input object without changing import order, pack choice, approval or independent-audio association. Remove unused test-only request helpers, keep independent checks independent, and resolve client/application lint findings without warning suppression or changing deadlines. Engine strict lint and the unresolved journal/startup reliability gates remain independent.

See [client storage and quality verification](PULSAR_CLIENT_STORAGE.md).


## ARCH-051: preserve engine feature boundaries and failed completion custody

Compile CUDA-specific session state, telemetry and relays with their backend, and test-only convenience wrappers with their tests. Production uses the existing lease/reader/staging-retaining entry points. Restore all-feature constructor ownership after protocol payload boxing. Return failed RGB completions in an owned boxed error retaining the original issued work, pixel allocations and admission. Verify allocation identity and release only after error disposal; preserve successful completion checks and serialized contracts. Resolve mechanical lint findings without warning suppression, removing custody or weakening deadlines.

See [engine feature boundaries](PULSAR_ENGINE_FEATURE_BOUNDARIES.md). Compile/build success does not qualify live CUDA startup, TensorRT inference, concurrency, hardware, model quality or release.


## ARCH-052: own native preparation separately from capture leases

Add explicit trusted-host native image preparation with an owner-bounded preparation deadline and separate retained-use lifetime. Keep verified bytes alive through a typed handle, with bounded watcher count and RAM admission, cancellation during preparation, timed expiry, explicit release and existing checked staging custody. Image availability is not permission, GPU readiness or physical cleanup. Every live launch still authenticates current intent and mounted bytes under its original deadline. Do not lengthen capture leases or turn offline jobs into live authority.

The Rust embedding foundation is implemented; authenticated protocol/controller preparation, durable status, cancellation/revocation and disposal remain delivery work. See [owned preparation](PULSAR_NATIVE_IMAGE_PREPARATION.md).

## ARCH-053 — Native preparation intent custody

Persist preparation intent and lifecycle metadata in the authority SQLite transaction before resource creation, under a conditional schema-27 reader barrier that preserves earlier nested readers. Bind intent to actor/project/revision/request/boot and image/policy identities; bound record count and bytes before parsing. Use generation-fenced transitions, prevent cancellation races from publishing late retained uses, and mark unfinished uses Interrupted on restart. Metadata never restores a handle or grants device/model authority; physical staging cleanup retains its existing independent custody.

Protocol/storage/startup foundations are implemented and tested. Authenticated asynchronous controller and first-party controls remain integration work; the trusted embedding method is not yet journal-backed. See [native preparation journal](PULSAR_NATIVE_PREPARATION_JOURNAL.md).


## ARCH-054: authenticate asynchronous native preparation ownership

Commit the original Start acknowledgement and intent before producer creation. Bind preparation to the current epoch, actor, project, revision and verified image/policy identities; carry one absolute preparation deadline through producer dispatch. Publish the producer's original retention clock, never a fresh host lease. Bound producer/monitor RAM and active operations; require current grants and revision during work, prevent cancelled or late completions from publishing retention, join before releasing producer admission, and confirm physical cleanup only when storage and fence custody are empty. Resolve lost replies without another producer. Expose the same operations through CLI and a non-authoritative desktop panel.

See [preparation control](PULSAR_NATIVE_PREPARATION_CONTROL.md). Authored file-custody verification does not qualify native GPU execution, desktop rendering, media quality, physical hardware or release.


## ARCH-055: preserve inert native preparation Start identities in clients

Before sending Start, durably record its original epoch, project/revision, request ID and options in bounded private storage bound to the session, credential identity and endpoint. Restore records for explicit resolution under current authority; never infer execution approval or automatically replay Start. Validate replies against frozen identity and preserve records after transport uncertainty. Local dismissal removes only metadata and does not cancel or release engine work. Reuse the shared owned, no-follow, single-link storage and nonblocking lock contract. See [client recovery](PULSAR_NATIVE_PREPARATION_CLIENT_RECOVERY.md).


## ARCH-056: withdraw exact model-pack approval and its generation

Bind generation to atomically recorded private admission roles instead of inferring pack authority from equal artifact bytes. Require current owner/session authority, an issued approval and the optimistic current selection fence. Withdraw current approval or preserve a newer selection when withdrawing historical approval. Cancel only matching unfinished generation; missing legacy roles require explicit broader scope. Commit state and the replay receipt before signalling captured handles, without releasing worker/resource custody. Preserve original recovery intent under the schema-3 client reader barrier. See [ADR B3-081](../research/phase-b/ADR-B3-081-model-pack-approval-withdrawal.md) and [verification and scope](PULSAR_MODEL_PACK_WITHDRAWAL.md). Automatic catalog-triggered revocation, live/Preview/native preparation effects and product qualification remain separate.


## ARCH-057: scope private recovery locks to the transaction

Hold advisory exclusion through callback and durable replacement, then explicitly unlock before success returns. A guard also attempts release on early error and unwinding, preventing an inherited descriptor from silently extending a completed transaction's lock. Keep nonblocking acquisition, private/no-follow/single-link/inode checks and original recovery identity semantics. Unlock refusal must not authorize dispatch. No schema or public capability changes. See [ADR B6-018](../research/phase-b/ADR-B6-018-transaction-scoped-recovery-lock-release.md) and [controlled regression and complete client results](PULSAR_JOURNAL_LOCK_RELEASE.md). Power-loss/platform and filesystem latency qualification remain separate.


## ARCH-058: keep learned assistant proposals separate from effects

Accept strict bounded proposals referring only to host-issued target/review/edit entries and explicitly allowed settings. Do not admit model-authored authority, arbitrary paths/code, device control or evidence. Fresh context, approval and existing protected edit/history transactions remain the future engine broker's responsibility. Preserve actual inference failures instead of selecting a default from syntax alone. Official Qwen2.5-0.5B Q4_K_M failed both initial synthetic campaigns; first-party assistant integration remains open. See [ADR B3-082](../research/phase-b/ADR-B3-082-local-assistant-proposal-foundation.md) and [candidate evidence](PULSAR_LOCAL_ASSISTANT_CANDIDATE.md).


## ARCH-059: execute learned assistant inference in a data-only worker

Use a bounded one-shot JSON role in the existing worker executable. Keep fixed guest runtime/model paths, verify mounted bytes, constrain output references to host scope and return original attempt/artifact-bound proposals. Preserve one native deadline, bounded egress and direct-child cleanup; the future authenticated Engine parent must own immutable assets, aggregate/cgroup/fence custody, cancellation, fresh-context approval and protected effect/history dispatch. Real Linux learned-role and refusal probes pass, while application/quality/installer/platform/device/human obligations remain open. See [ADR B3-083](../research/phase-b/ADR-B3-083-one-shot-learned-assistant-worker.md) and [worker evidence](PULSAR_LOCAL_ASSISTANT_WORKER.md).


## ARCH-060: retain assistant assets and admission until whole-tree retirement

The trusted Engine embedding API captures exact worker/runtime/model bytes into sealed images under shared aggregate admission and durable worker custody. Keep fixed guest paths, isolated execution, original deadline/cancellation and independent whole-response identity/scope checks. Install asset ownership before fallible launch and release only after confirmed tree/group/fence retirement. Authenticated client workflow, fresh application approval/effects, memory handoff and release qualification remain open. See [ADR B3-084](../research/phase-b/ADR-B3-084-engine-owned-assistant-execution.md) and [bounded evidence](PULSAR_LOCAL_ASSISTANT_PARENT.md).


## ARCH-061: authenticate and retain assistant proposals independently from effects

Use host-registered lifetime profiles, bounded client text, Engine-built current context and Read/Edit revision fences. Commit original attempt receipts before dispatch; keep exact retry observational. Commit cancellation/revocation/profile withdrawal before captured signals, preserve failed completion publication for metadata-only retry, and interrupt without automatic execution on restart. Actual action approval, source/view context, protected undo, first-party/offline delivery and qualification remain open. See [ADR B3-085](../research/phase-b/ADR-B3-085-authenticated-assistant-attempts.md) and [request-path evidence](PULSAR_AUTHENTICATED_ASSISTANT.md).


## ARCH-062: configure assistant exceptions privately and preserve first-party start intent

Require bounded private explicitly approved startup policy, exact captured worker/native declarations and owner resource feasibility. Keep paths host-only and registration lifetime-scoped. Prepare private exact endpoint/session/credential-bound start metadata before CLI dispatch; require inference approval and reuse the original identity/declaration on retry. Existing engine ownership, durable synchronization and project effects remain separate. See [ADR B3-086](../research/phase-b/ADR-B3-086-private-assistant-host-policy-and-cli.md) and [actual flow/failure evidence](PULSAR_ASSISTANT_HOST_AND_CLI.md).


## ARCH-063: bind desktop assistant approval to a durable exact request

Replace the disabled desktop placeholder with the authenticated proposal panel. Save private original request metadata before approval, retain a semantic digest and dispatch a single owned checked intent. Bind background work/replies to project/revision, clear disposable approvals on view changes, preserve exact retry identity and keep cancellation separate from inference approval. Display proposals without effect or device authority. Native rendering, learned desktop execution, protected application/undo, handoff and delivery qualification remain open. See [ADR B3-087](../research/phase-b/ADR-B3-087-desktop-assistant-intent-and-view-bindings.md) and [desktop evidence](PULSAR_ASSISTANT_DESKTOP.md).


## ARCH-064: resolve assistant acceptance or permanently withdraw an unpublished ID

Keep exact original private intent across uncertain Start acknowledgements. Authenticate Read/Edit, validate the full accepted declaration, or reserve the unused original namespace atomically with a resolution receipt. Historical resolution never executes inference or cancellation; immutable replay and fresh status remain separate. CLI/desktop explicitly disclose permanent withdrawal. See [ADR B3-088](../research/phase-b/ADR-B3-088-assistant-start-resolution.md) and [source-bound evidence](PULSAR_ASSISTANT_START_RESOLUTION.md).


## ARCH-065: approve a retained assistant stride for a specific generation request

Require fresh authenticated Read/Edit/Generate and exact attempt/context/proposal binding. Derive existing Generate settings while retaining the original approval command as its transactional receipt identity. Recheck after source/model capture; keep candidate commit/protection/history separate. See [ADR B3-089](../research/phase-b/ADR-B3-089-assistant-approved-generation.md) and [behavior/evidence](PULSAR_ASSISTANT_GENERATION_APPROVAL.md).


## ARCH-066: resolve assistant review selections to exact recorded evidence

Capture bounded current-master review flags with optional exact-ID focus. Preserve reasons, evidence, absent confidence and lineage uncertainty. Authenticate a fresh ReviewStatus read before displaying a completed selected flag, bind desktop cache to view/attempt, and reconstruct identical context for stride approvals. See [ADR B3-090](../research/phase-b/ADR-B3-090-assistant-recorded-review-evidence.md) and [contract/evidence](PULSAR_ASSISTANT_REVIEW_EXPLANATIONS.md).


## ARCH-067: bind saved generation approval to exact first-party dispatch and job handoff

Capture the complete assistant generation command and original ID in the checked private store. Verify the displayed stride's proposal digest; reset approval on restore and recheck the reviewed file before desktop dispatch. Preserve pending mutation identity across read-only replies and refuse displacement. Reuse existing Engine replay/current-authority validation and desktop job/candidate/protected-commit controls. See [ADR B3-091](../research/phase-b/ADR-B3-091-assistant-generation-intent-and-desktop-handoff.md) and [source-bound evidence](PULSAR_ASSISTANT_GENERATION_INTENTS.md).

## ARCH-068: retain checked preview target references before assistant application

Bind inert observed target IDs to accepted preview frame/model/context/view facts in a bounded session/project/revision registry. Require unambiguous box centres and retain no inferred class identity. Authenticated read-only queries expose current references; Start context, approval application and target-aware desktop/saved intents remain separate unfinished work. See [ADR B3-092](../research/phase-b/ADR-B3-092-checked-assistant-target-catalog.md) and [capture contract](PULSAR_ASSISTANT_TARGET_CATALOG.md).

## ARCH-069: bind target proposal application to retained observed facts

Capture explicit target focus from the checked session/project/revision catalogue; recompute those facts before approved generation. Derive the unclassified seed/view and refuse conflicting declarations while preserving normal generation and exact receipt replay. Saved requests and desktop approval bind the displayed stride or target ID to the proposal digest. See [ADR B3-093](../research/phase-b/ADR-B3-093-assistant-target-application.md) and [software/qualification scope](PULSAR_ASSISTANT_TARGET_APPLICATION.md).

## ARCH-070: apply bounded assistant edits through authored candidates

Bind fixed values-only recipe IDs to current project/revision/motion and explicit local assistant focus. Separate proposal approval from inference and master commit. Require Read/Edit authority, reconcile synthesized provenance and protected segments, and atomically register candidate/lineage/original receipt. Existing commit and undo/redo remain authoritative. See [ADR B3-094](../research/phase-b/ADR-B3-094-assistant-edit-recipes.md) and [software/qualification scope](PULSAR_ASSISTANT_EDIT_RECIPES.md).

## ARCH-071: persist exact assistant edit approvals for manual recovery

Capture the full checked recipe and original request/revision/approval in the existing private intent store. Restore without mutation, reset approval, and reload/check the reviewed digest and credential namespace at dispatch. Preserve pending identity and expose historical accepted candidate/base revision. Fresh execution keeps ordinary Engine freshness checks; commit stays separate. See [ADR B3-095](../research/phase-b/ADR-B3-095-saved-assistant-edit-recovery.md) and [recovery scope](PULSAR_ASSISTANT_EDIT_RECOVERY.md).

## ARCH-072: lend owned generation admission to authenticated assistant work

Require current authority and exact immutable input checkpoints before transactionally pausing same-owner/project offline generation. Signal only after commit, wait for actual custody release, and transfer the reservation into supervised assistant launch. Resume marked jobs through ordinary authenticated admission with the same job ID and a new attempt. Keep live/preview/device work outside the victim set, and distinguish input-boundary restart from partial computation recovery. See [ADR B3-096](../research/phase-b/ADR-B3-096-assistant-memory-handoff.md) and [handoff scope](PULSAR_ASSISTANT_MEMORY_HANDOFF.md).

## ARCH-073: bound assistant journal capture before decoding

Share typed report capture across startup, lookup, lifecycle controls and admission. Require bounded text identities/report cells, a capped catalogue, exact stored/report identity agreement and bounded JSON publication including escaping. Refuse damaged records before restart or cancellation effects; preserve explicit recovery. See [ADR B3-097](../research/phase-b/ADR-B3-097-assistant-journal-capture-admission.md) and [journal admission scope](PULSAR_ASSISTANT_JOURNAL_ADMISSION.md).

## ARCH-074: wake assistant recovery after external capacity release

Use one weak Engine recovery thread and bounded coalesced resource-release notifications. Subscribe before retry, suppress self-generated wakes, and fence concurrent completion at shutdown. Keep inference singular and ordinary current-authority/checkpoint/native admission authoritative for resume. See [ADR B3-098](../research/phase-b/ADR-B3-098-assistant-capacity-release-recovery.md) and [software/native scope](PULSAR_ASSISTANT_CAPACITY_RECOVERY.md).


## ARCH-075: relocate explicitly approved assistant assets

Keep legacy absolute schema-1 owner policies and introduce schema-2 relative runtime/model declarations anchored to the explicitly selected private policy. Refuse traversal, absolute paths, missing files and symlinks; retain exact-byte capture, approval and resource admission. No automatic model selection or inference is added. See [ADR B3-099](../research/phase-b/ADR-B3-099-portable-assistant-policy-assets.md) and [portable policy scope](PULSAR_PORTABLE_ASSISTANT_POLICY.md).


## ARCH-076: measure and publish local assistant host policy

Intercept first-party setup in the executable host before Engine discovery. Keep grammar in clients and local hashing, profile admission and exclusive private publication in the trusted host API. Do not turn host setup into RPC authority or automatic inference/effects. See [ADR B3-100](../research/phase-b/ADR-B3-100-local-assistant-host-setup.md) and [setup scope](PULSAR_ASSISTANT_HOST_SETUP.md).


## ARCH-077: discover bundled CPU inference runtime

Preserve explicit owner runtime overrides; otherwise pin the executable-relative platform runtime if present. Missing bytes remain optional; invalid entries and metadata errors surface. Discovery never loads inference or selects a model. Retain captured identities, profile/pack correspondence and confined worker execution. Linux assembly carries exact runtime/license/notices pins and records actual system dependency names with unqualified status. See [ADR B3-101](../research/phase-b/ADR-B3-101-bundled-cpu-runtime-discovery.md).


## ARCH-078: publish offline runtime installations atomically

Intercept runtime-install in the trusted executable host before Engine configuration. Capture a bounded finite candidate manifest and exact regular bytes through held no-follow directories; flush a private owned stage before atomic no-overwrite publication. Never overwrite an existing destination or activate inference. Process-death stages require explicit inspection; final parent-sync uncertainty is surfaced. See [ADR B3-102](../research/phase-b/ADR-B3-102-offline-runtime-candidate-installation.md).


## ARCH-079: inspect and explicitly discard recognized installation stages

Lock an integrity-bound private local receipt before copying payloads. Require current owner/path/receipt checks, finite full-inventory preflight, explicit approval and exact inspected manifest identity before descriptor-relative removal. Refuse active locks and unknown/damaged contents; preserve legacy unmarked stages. Published destination basenames remain outside stage discard. See [ADR B3-103](../research/phase-b/ADR-B3-103-runtime-installation-stage-recovery.md).


## ARCH-080: transport offline runtime candidates in a bounded single file

Keep original finite manifest bytes and payload record order in a versioned uncompressed archive. Stream exact lengths/hashes through held descriptors, reject malformed/truncated/trailing/changed contents and publish archives without overwrite. Reuse existing installation stage custody for extraction; no Engine startup, model activation or archive-temp automatic deletion. See [ADR B3-104](../research/phase-b/ADR-B3-104-offline-runtime-archive.md).


## ARCH-081: launch persistent verified assets before transient distribution teardown

Install each original candidate manifest into a separate private cache directory. Serialize preparation and verify the exact complete cache on every launch; refuse damaged/unknown entries without replacement. AppImage AppRun executes the cached application before Engine startup so detached Engine/worker/runtime paths survive extraction teardown. Preserve owner selections and approval boundaries. See [ADR B3-105](../research/phase-b/ADR-B3-105-persistent-appimage-runtime-custody.md).


## ARCH-082: require observed filesystem cleanup from the AppImage runtime

Treat a success code without removal as a failed cleanup gate. Pin the downstream runtime source/patch/build bytes and use a positive bounded nftw descriptor limit under musl. Require actual automatic extraction removal before continuing cached Engine/worker checks; never substitute fixture deletion for the passing gate. See [ADR B3-106](../research/phase-b/ADR-B3-106-appimage-musl-cleanup.md).

## ARCH-083: present graphical installation before Engine startup

Use a local setup window for valid graphical packaged launches. Run the executable's ordinary installer callback off the event loop; show bounded errors, explicit retry and close. Discard late results on close and terminate the bootstrap process before any cached launch. Headless commands retain their existing path. See [ADR B3-107](../research/phase-b/ADR-B3-107-runtime-bootstrap-window.md) and [qualification scope](PULSAR_RUNTIME_BOOTSTRAP_GUI.md).

## ARCH-084: explicitly select verified retained runtime assets

Admit one supplied manifest identity using existing trusted cache components and lock, then validate schema, receipt, inventory and every payload before inspection or execution. Preserve every other entry and existing Engine; no automatic version selection, repair, active pointer or database rollback. See [ADR B3-108](../research/phase-b/ADR-B3-108-explicit-retained-runtime-selection.md) and [operation scope](PULSAR_RETAINED_RUNTIME_SELECTION.md).

## ARCH-085: keep saved assistant settings in the trusted executable host

Require explicit private saved selection and exact-digest disable for subsequent startup, with an explicit environment override retained. Bind desktop settings callbacks to their configured state/resource context and dispatch one approved request off the event loop. Client presentation cannot write host policy or acquire Engine/device authority. Keep native rendered GUI and useful-default acceptance independent from headless/controller results. See [ADR B3-137](../research/phase-b/ADR-B3-137-persistent-assistant-host-selection.md), [ADR B3-138](../research/phase-b/ADR-B3-138-desktop-assistant-host-settings.md), and [scoped evidence](PULSAR_DESKTOP_ASSISTANT_SETTINGS.md).


## ARCH086 — explicit causal spectral audio analysis

Put checked band/threshold values and bounded streaming DSP in Core, the optional declaration in Protocol, strict source decoding and evidence in Worker, and controls in Clients. Omitted analysis preserves existing full-band mapping. Causal delay remains uncompensated and saturation/gating is disclosed. Six-axis bundles and musical-beat/media/device acceptance remain separate. See [ADR B3-139](../research/phase-b/ADR-B3-139-spectral-audio-analysis.md).


## ARCH087 — bounded audio-modulated carrier

Declare canonical Vibe mode in Protocol and derive its 8 Hz carrier in pure Core from checked RMS features. Preserve source timestamps, retain leading gaps, reject sparse envelopes, and disclose relative phase/range/silence policy in Worker lineage. Share mode selection across CLI and desktop; historical aliases are CLI grammar only. Deterministic synthesized output gives no device or motion-quality authority. See [ADR B3-140](../research/phase-b/ADR-B3-140-audio-modulated-oscillation.md).


## ARCH088 — explicit neutral-script repair intake and values

Make sanitation an explicit repair-import variant under the existing immutable source, admission and ImportSource+Edit scope. Ordinary import stays strict. Keep bounded neutral-grid jitter/slope math in Core, then use the existing untrusted edit broker, receipts, protected-segment checks and history for the CLI workflow. Preserve raw input and refuse existing outputs. Script slopes are numerical constraints, not physical admission. See [ADR B3-141](../research/phase-b/ADR-B3-141-shared-script-repair.md) and [scoped evidence](PULSAR_SCRIPT_REPAIR.md).

## ARCH089 — bind desktop repair proposals to captured scope

Capture checked Core policy and an immutable committed program with project/revision, then calculate and upload on the existing RPC thread. Refuse missing/rebound work before effects, validate returned candidate scope and numerical values, and discard late replies from a changed view while retaining their candidate ID for inspection. Candidate review and explicit commit retain existing history and protected-segment authority. Headless/controller evidence does not qualify the rendered GUI. See [ADR B3-142](../research/phase-b/ADR-B3-142-desktop-script-repair.md) and [scope](PULSAR_DESKTOP_SCRIPT_REPAIR.md).

## ARCH090 — publish complete Linux recovery metadata atomically

Write and sync a private held unnamed inode, then link without replacement and validate/sync the existing receipt scope. Keep handled cleanup guards and refuse unsupported filesystem/procfs configurations explicitly. Abrupt process death is separate from power-loss durability; legacy truncated receipts and other platforms stay open. See [ADR B3-143](../research/phase-b/ADR-B3-143-atomic-linux-receipts.md) and [scope](PULSAR_ATOMIC_LINUX_RECEIPTS.md).

## ARCH091 — present explicit Handy observations through the shared broker

Keep credential values and endpoint selection out of the desktop. Capture names and per-request online consent, load the separate private owner proof on the session-bound RPC thread and use the existing read-only Engine operation. Validate typed reports, clear stale observations/consent on new requests or name changes, and retain physical qualification separately. See [ADR B3-144](../research/phase-b/ADR-B3-144-desktop-handy-inspection.md) and [scope](PULSAR_DESKTOP_HANDY_INSPECTION.md).

## ARCH092 — match cross-platform source scope to actual contracts

Keep Unix-only publications and Linux-only cgroup qualification fixtures scoped to those implementations, give optional prepared source receipt data a stable explicit type, and retain portable database assertions. Use cross-target compilation and linked portable tests as separate evidence: Windows emulation failures remain open and cannot establish native/MSVC release qualification. See [ADR B3-145](../research/phase-b/ADR-B3-145-windows-source-boundaries.md) and [scope](PULSAR_WINDOWS_SOURCE_CHECK.md).


## ARCH093 — validate paths in their declared namespace

Keep native host supplier paths separate from exact POSIX guest capture coordinates, sharing bounded dependency metadata checks. Admit native prefixes in bundled presentation only within the existing normalized absolute/digest-child checks. Make positive host fixtures portable and test non-Unix package refusal explicitly, retaining supported Unix custody tests and immutable wire fixtures. Emulated portable success grants no native Windows or release qualification. See [ADR B3-146](../research/phase-b/ADR-B3-146-host-and-guest-path-domains.md) and [scope](PULSAR_WINDOWS_PORTABLE_FIXTURES.md).


## ARCH094 — retain checked Windows package bytes without granting authority

Open selected Windows filesystem input with safe read-only, no-write/delete-sharing and final-reparse semantics; check held metadata before hashing. Keep shared bounded byte identity, version, replay and upload logic. Exercise real handles and refusal/release behavior, while retaining Windows Engine private storage/publication and native filesystem qualification separately. See [ADR B3-147](../research/phase-b/ADR-B3-147-windows-retained-package-input.md) and [scope](PULSAR_WINDOWS_PACKAGE_INPUT.md).


## ARCH095 — refuse unestablished artifact guarantees before effects

Require an actual platform implementation for private directories and namespace durability. Stop unsupported snapshot/reuse/export paths before creating or publishing bytes. Keep Windows supplier reads separate, using native read-only sharing/reparse and held metadata checks. Retain the failing baseline and scoped final evidence without substituting emulation for native Windows qualification. See [ADR B3-148](../research/phase-b/ADR-B3-148-checked-windows-artifact-boundaries.md) and [scope](PULSAR_WINDOWS_ARTIFACT_BOUNDARIES.md).


## ARCH096 — compare observable change metadata throughout capture and reuse

Use one successful-mtime/length predicate with Unix nanosecond ctime checks at fresh capture, worker-source rechecks and retained-object verification. Retain the actual same-length rewrite/restored-mtime failing control and scoped final evidence. Timestamp equality is not an atomic-source guarantee or platform qualification. See [ADR B3-149](../research/phase-b/ADR-B3-149-observable-source-change-metadata.md) and [scope](PULSAR_SNAPSHOT_CHANGE_DETECTION.md).

## ARCH097 — check native completions against advertised choices

Share one sampling-aware creative palette between the grammar and independent decoded-parameter validation. Preserve Core/scope checks and explicit generation approval. Retain the existing production context after measured expanded/compact presentations regress; numerical admission does not prove natural-language fidelity. See [ADR B3-150](../research/phase-b/ADR-B3-150-native-creative-palette.md) and [scope](PULSAR_NATIVE_CREATIVE_CAPABILITIES.md).

## ARCH098 — revalidate creative capabilities at approval

A matching retained report and exact approval do not prove native numerical capability admission. Reapply the shared palette after authority/context and Core/scope checks, before deriving generation; refusal must leave jobs, project and replay identity unchanged. Keep explicit pattern input's broader Core contract. See [ADR B3-151](../research/phase-b/ADR-B3-151-creative-approval-palette.md) and [scope](PULSAR_CREATIVE_APPROVAL_CAPABILITIES.md).

## ARCH099 — captured-source metadata queries

Standalone inspection captures a source through project authority, then uses an exact-revision isolated worker query with bounded metadata, unique leases and no generated artifact egress. Metadata never claims decoded or qualified media. Scope/revision checks cover admission and post-execution; probe leases participate in eviction protection and post-commit revocation. See [ADR B3-152](../research/phase-b/ADR-B3-152-media-inspection-query.md) and [inspection scope](PULSAR_MEDIA_INSPECTION.md).

## ARCH100 — shared CLI tier admission and bounded Maximum background recovery

Public generation uses the existing exact visual recipe contract before project/source effects and preserves original-frame stride/time semantics. Maximum background recovery adds distinct geometrically supported points while retaining its original grid, target exclusion, strict Core checks and 64-point budget; the versioned plan and receipt sampling disclose the change. Default/Fast are unchanged. See [ADR B3-153](../research/phase-b/ADR-B3-153-cli-visual-tiers.md) and [scope](PULSAR_CLI_VISUAL_TIERS.md).

## ARCH101 — explicit model declarations for folder consumers

Batch and scan --generate share checked declarations and the existing revision-bound file-generation path. Validate the ABI/profile/settings before discovery or project effects; retain Engine pinning/execution authority, companion skipping and dry-run JSON compatibility. No implicit model selection or annotation is introduced. See [ADR B3-154](../research/phase-b/ADR-B3-154-library-model-generation.md) and [scope](PULSAR_LIBRARY_MODEL_GENERATION.md).


## ARCH102 — explicit six-axis audio bundle inputs

One immutable source and exact six-axis checked recipe reach shared per-axis DSP after one decode, producing one synthesized candidate/commit with original clock and per-axis lineage. CLI publishes individual selected-axis exports into a new directory and reports partial failures with retained project/revision; no aggregate atomicity or physical meaning is claimed. See [ADR B3-155](../research/phase-b/ADR-B3-155-audio-bundle.md) and [scope](PULSAR_AUDIO_BUNDLE.md).


## ARCH103 — checked desktop audio bundle authoring

Separate single-axis and six-axis drafts in a presentation-only panel; retain legacy single-axis defaults and use explicit checked per-axis recipes for bundles. Disable invalid/missing-source/busy submissions, capture source/settings by value and send through existing exact project/revision authority. Proposals require separate review/commit; no export or physical meaning is implied. See [ADR B3-156](../research/phase-b/ADR-B3-156-desktop-audio-bundle.md) and [scope](PULSAR_DESKTOP_AUDIO_BUNDLE.md).


## ARCH104 — atomic neutral bundle publication and exact request recovery

A shared authenticated helper stages six existing Engine axis exports with durable named-axis/request identities, exact master revision and stable held-file/directory checks. Bounded independent neutral-action verification and sync precede one no-replace rename. Retained staging enables authorized receipt replay; unknown outcomes refuse. Linux/Android support does not qualify other platforms or physical durability. See [ADR B3-157](../research/phase-b/ADR-B3-157-neutral-bundle-publication.md) and [scope](PULSAR_NEUTRAL_BUNDLE_PUBLICATION.md).


## ARCH105 — desktop atomic six-axis export and explicit recovery

Capture selected project/revision and output path through the shared transfer lane; resume retained authenticated intent. Preserve typed outcome/staging, block uncertain publication pending operator disposition and drain local stop ownership. Use indeterminate progress. See [ADR B3-158](../research/phase-b/ADR-B3-158-desktop-neutral-bundle.md) and [scope](PULSAR_DESKTOP_NEUTRAL_BUNDLE.md).


## ARCH106 — exact selected-pack folder generation

Folder consumers capture the authenticated selected pack/profile/revision once, derive declared input settings and call shared single-file pack generation for every pending file. Engine revalidates authority and selection at admission. No automatic selection/default/fallback or overwrite. See [ADR B3-159](../research/phase-b/ADR-B3-159-library-selected-packs.md) and [scope](PULSAR_LIBRARY_PACKS.md).


## ARCH107 — cleanup receipt lock owner lifetime

Cleanup receipt Records explicitly end their lock ownership at retirement, preserving active-owner refusal and all binding/identity/deletion guards. An unrelated inherited descriptor cannot extend the ended Record lifetime. See [ADR B3-160](../research/phase-b/ADR-B3-160-cleanup-lock-owner-lifetime.md) and [scope](PULSAR_CLEANUP_LOCK_OWNERSHIP.md).


## ARCH108 — explicit six-axis audio folder generation

Folder consumers use the checked per-axis audio recipe and existing single-file synthesis/atomic publisher. Full-filename targets preserve companions and avoid same-stem collisions; existing target contents remain unverified and skipped. Immediate per-entry receipts survive later failure. No visual/physical reconstruction or aggregate durable queue is implied. See [ADR B3-161](../research/phase-b/ADR-B3-161-library-audio-bundles.md) and [scope](PULSAR_LIBRARY_AUDIO_BUNDLES.md).


## ARCH109 — observational neutral bundle inspection

Authenticated retained-stage inspection reuses exclusive ownership and exact master/file comparison. Missing files are reported; unsafe or mismatched present files refuse. Matching local bytes never establish Engine export outcomes or authorize retry. Publication/resume remain separate. See [ADR B3-162](../research/phase-b/ADR-B3-162-neutral-bundle-inspection.md) and [scope](PULSAR_NEUTRAL_BUNDLE_INSPECTION.md).


## ARCH110 — retained local input and generation ownership

Engine-owned local batches freeze source bytes and checked audio recipes, protect retained snapshot membership and persist exact mutation/advance identities before effects. Explicit current-session advancement reuses the shared project/import/job/candidate/commit contracts. Acknowledgements and outer replies publish atomically; restart dispatches no work. Schema 28 wraps earlier reader layers, whose inner writers preserve that header. Generation readiness remains separate from atomic publication and qualification. See [ADR B3-163](../research/phase-b/ADR-B3-163-local-batch-generation-ledger.md) and [scope](PULSAR_LOCAL_BATCH_GENERATION.md).


## ARCH111 — explicit local batch metadata release

Explicit authenticated release frees unstarted or generation-ready ledger membership and pins, with an atomic exact-request receipt. Partial generation, pending acknowledgements and item errors refuse release. Source/media/output files, downstream project/job/export records and other batch pins remain preserved; no publication or qualification claim follows. See [ADR B3-164](../research/phase-b/ADR-B3-164-local-batch-release.md) and [scope](PULSAR_LOCAL_BATCH_GENERATION.md). Full roadmap PARTIAL; aggregate publication scheduling and model/media/platform/device/human/release gates remain open.


## ARCH112 — retained local batch publication handoffs

Explicit client prepare/publish/status connects retained generation to the existing neutral publisher. A private journal retains exact stage/export request and parent/stage filesystem identities before effects, and actual publisher receipts per entry afterward. Unknown acknowledgements refuse replacement/adoption; generation and publication history remain distinct. See [ADR B3-165](../research/phase-b/ADR-B3-165-local-batch-publication-handoff.md) and [scope](PULSAR_LOCAL_BATCH_PUBLICATION.md). Full roadmap PARTIAL; unified scheduling, unresolved acknowledgement disposition, model/media/platform/device/human/release gates remain open.


## ARCH113 — bounded retained local batch scheduling

Explicit run/status connects captured input, retained generation and per-entry publication. Private schema-2 history is stored before generation; immutable/acquired identities cannot regress or change. Ready entries publish before later generation, read-only job observation avoids durable poll replies, and failures preserve earlier receipts without replacement jobs. See [ADR B3-166](../research/phase-b/ADR-B3-166-local-batch-scheduler.md) and [scope](PULSAR_LOCAL_BATCH_SCHEDULER.md). Full roadmap PARTIAL; continuous watching/intake, cancellation/disposition/migration/GC, unknown acknowledgements and model/media/platform/device/human/release gates remain open.


## ARCH114 — durable local batch cancellation and partial metadata release

Explicit owner-scoped cancellation durably stops new batch generation, retains exact known-job cancellation identities and reconciles pending generation only from existing positive cache receipts. Unknown acknowledgements remain pending; no replacement work or output rollback is supplied. Completed cancelled partial batches can release only metadata memberships after terminal-job and absent-worker-handle checks. Scheduler/publication observe stop policy and retain prior receipts. The outer reader floor is 29; inner storage writers preserve it. See [ADR B3-167](../research/phase-b/ADR-B3-167-local-batch-cancellation.md), [contract](PULSAR_LOCAL_BATCH_CANCELLATION.md) and [checkpoint](evidence/roadmap-2026-10-12/library-cancellation/checkpoint.json).

Selected-package tests pass 1,522 with 80 ignored; final clients, both strict lints, source/platform/build/format/architecture checks and native cancellation/scheduler campaigns pass. The older reader refuses and preserves database/WAL bytes; its shared-memory sidecar preservation assertion remains FAIL. Cancellation acknowledges software stop, not physical worker retirement. Full roadmap PARTIAL; unknown-receipt disposition, migration/collection, continuous watching/intake and model/media/GUI/platform/device/durability/human/release gates remain open.


## ARCH115 — desktop retained audio library workflow

The desktop Library batches tab now exposes retained audio-recipe intake, owned catalog/status, shared bounded generation/publication, local pause between decisions, exact mutation recovery, durable cancellation, metadata release and retained project opening. Native controller IPC and the final CLI scheduler regression pass; twelve outputs match retained PCM/timing oracles. Client tests pass 366 with ten ignored; both strict lint configurations and remaining scoped checks pass. Headless egui tests do not establish rendered GUI acceptance. Visual/model folder controls and all independent platform/model/media/device/human/release gates remain open. See [ADR B3-168](../research/phase-b/ADR-B3-168-desktop-library-batches.md), [contract](PULSAR_DESKTOP_LIBRARY_BATCHES.md) and [checkpoint](evidence/roadmap-2026-10-12/desktop-library/checkpoint.json).


## ARCH116 — shared desktop audio recipe authoring

The retained desktop audio workflow now uses the same per-axis recipe controls as single-file Audio generation, with explicit JSON intake retained. Direct capture snapshots checked settings before background discovery and retains full original options for exact recovery; local discovery failures remain editable. Both native editor/file workflows pass and all 24 exported scripts match preceding authored PCM/timing oracles. Client tests pass 370 with ten ignored; both strict lint configurations and remaining affected checks pass. Rendered GUI acceptance, visual/model folder workflow, broader input/edition integration and independent model/media/platform/device/human/release gates remain open. See [ADR B3-169](../research/phase-b/ADR-B3-169-library-audio-recipe-controls.md), [contract](PULSAR_LIBRARY_RECIPE_AUTHORING.md) and [checkpoint](evidence/roadmap-2026-10-12/library-recipe-controls/checkpoint.json).


## ARCH117 — original retained batch preparation recovery

Explicit CLI preparation recovery now binds the exact retained stage to its original private pending receipt and the current journal transition. It restores Prepared metadata without generation/export dispatch; missing or mismatched original evidence refuses. Actual client death after pending-receipt fsync and before record replacement, exact repeat/restart and four native refusals pass; twelve outputs match retained PCM/timing oracles. Client tests pass 373 with ten ignored; both strict lint configurations and remaining affected checks pass. Desktop recovery controls, broader faults and independent qualification remain open. See [ADR B3-170](../research/phase-b/ADR-B3-170-original-batch-preparation-recovery.md), [contract](PULSAR_LIBRARY_PREPARATION_RECOVERY.md) and [checkpoint](evidence/roadmap-2026-10-12/library-preparation-recovery/checkpoint.json).


## ARCH118 — desktop original preparation recovery

The desktop library controller now exposes exact original preparation recovery with captured batch/directory/index/stage/receipt inputs and fresh history. Known preflight refusals remain editable; uncertain metadata writes retain the original action and block replacement mutations. Actual client-death recovery, owned read-only write refusal/exact retry and desktop replay after Engine restart pass; twelve outputs match retained timing oracles. Client tests pass 375 with ten ignored; both strict lint configurations and remaining affected checks pass. Rendered GUI acceptance and broader independent gates remain open. See [ADR B3-171](../research/phase-b/ADR-B3-171-desktop-preparation-recovery.md), [contract](PULSAR_DESKTOP_PREPARATION_RECOVERY.md) and [checkpoint](evidence/roadmap-2026-10-12/desktop-preparation-recovery/checkpoint.json).


## ARCH119 — selected neutral-axis publication

Explicit selected-axis publication now supports one through six committed neutral tracks through CLI and desktop shared controls. Full six-axis schema 1 is preserved; subset schema 2 retains original named request identities and is refused by the predecessor client. Master/member preflight runs before export dispatch, and final strict-byte/revision verification and no-replace rename remain authoritative. Client tests pass 380 with ten ignored; both strict lint configurations, Windows source check, native build, formatting and architecture/source guards pass. Native client-death/restart recovery, older-reader refusal, missing/duplicate/schema/extra-member refusals and one-/two-axis byte comparisons pass. The initial compatibility-test and native preflight failures are retained and corrected. Rendered GUI acceptance, retained visual/model batch workflow and independent model/media/platform/device/human/release gates remain open. See [ADR B3-172](../research/phase-b/ADR-B3-172-selected-axis-publication.md), [contract](PULSAR_SELECTED_AXIS_PUBLICATION.md) and [checkpoint](evidence/roadmap-2026-10-12/selected-axis-publication/checkpoint.json).

## ARCH120 — retained visual/model library batches

Retained visual/model batches now share CLI and desktop JSON intake, captured source/model bytes, bounded generation, selected Stroke publication, exact restart recovery, cancellation and metadata release. Visual report schema 3 arms storage reader floor 30; preceding Engines refuse it. The native campaign produces four scripts matching the retained detector fixture and preserves existing outputs. Engine/Protocol/Clients suites pass 883/284/382 tests, with 70/0/10 ignored. Both strict lint configurations, Windows source check, builds, formatting and architecture/source guards pass. Selected model-pack recipes, direct visual recipe authoring, rendered GUI acceptance and independent model/media/platform/device/human/release qualification remain open. Full roadmap PARTIAL. See [ADR B3-173](../research/phase-b/ADR-B3-173-retained-visual-batches.md), [contract](PULSAR_RETAINED_VISUAL_BATCHES.md) and [checkpoint](evidence/roadmap-2026-10-12/retained-visual-batches/checkpoint.json).

## ARCH121 — direct desktop visual library recipe controls

The desktop Library batches panel now authors visual Stroke recipes directly, with explicit file-model paths, declared profiles or visible YOLO tensor declarations, preset/dimension/threshold controls and source sampling stride. Capture freezes the form before background profile loading and media discovery; successful intake retains exact Prepare content for retries. Client tests pass 385 with ten ignored; both strict lint configurations, Windows source check, native/helper builds, formatting and architecture/source guards pass. The actual native desktop controller generates two scripts matching the retained detector fixture; the two-output CLI regression also passes, including restart, exact repeat, cancellation/release and existing-output preservation. Rendered-window acceptance, selected model-pack recipes, advanced direct view authoring, broader intake/watching/fault coverage and independent model/media/platform/device/human/release gates remain open. Full roadmap PARTIAL. See [ADR B3-174](../research/phase-b/ADR-B3-174-library-visual-controls.md), [contract](PULSAR_LIBRARY_VISUAL_CONTROLS.md) and [checkpoint](evidence/roadmap-2026-10-12/library-visual-controls/checkpoint.json).

## ARCH122 — selected packs in retained local library batches

Retained visual library batches now admit exact owner-selected model packs through CLI and desktop JSON intake. Preparation pins the model plus manifest/profile/approval documents, enriches checked settings and rechecks the selection fence at publication. Each future job submits GenerateFromModelPack with the original choice and profile, preserving ordinary fresh admission. Report/publication schema 4 and reader floor 31 prevent older interpretation; document memberships remain pinned until release. Engine/Protocol/Clients suites pass 884/285/386 tests with 70/0/10 ignored. Both strict lint configurations, Windows source check (with existing warnings), native/helper builds, formatting and architecture/source guards pass. Four native scripts match the retained detector fixture; restart/repeat, predecessor refusal, cancellation/release, captured-document corruption and deactivation/reselection refusal with no replacement jobs pass. Direct pack controls, stopped-batch migration and unknown-effect disposition, continuous watching, broader faults and rendered GUI/model/media/platform/device/human/release qualification remain open. Full roadmap PARTIAL. See [ADR B3-175](../research/phase-b/ADR-B3-175-library-model-packs.md), [contract](PULSAR_LIBRARY_MODEL_PACKS.md) and [checkpoint](evidence/roadmap-2026-10-12/library-model-packs/checkpoint.json).

## ARCH123 — direct desktop selected-pack library controls

The desktop visual library editor now loads the current owner-selected pack in a bounded background operation and captures an explicit digest, preset, dimensions, threshold and sampling stride. Capture queries the current choice/profile again before discovery and retains fully parsed Prepare content for exact retries; refresh cannot rewrite pending preparation. No import, selection or approval is performed. Client tests pass 392 with ten ignored; both strict lint configurations, Windows source check (with existing warnings), native/helper builds, formatting and architecture/source guards pass. Native controller refresh/capture/publication, wrong-digest refusal without a batch, and the CLI restart/repeat/cancellation/corruption/deactivation/reselection regression pass; four scripts match the retained detector fixture. Stopped-batch migration, unknown-effect disposition, advanced views, continuous watching, broader faults and rendered GUI/model/media/platform/device/human/release qualification remain open. Full roadmap PARTIAL. See [ADR B3-176](../research/phase-b/ADR-B3-176-library-pack-controls.md), [contract](PULSAR_LIBRARY_PACK_CONTROLS.md) and [checkpoint](evidence/roadmap-2026-10-12/library-pack-controls/checkpoint.json).


## ARCH124 — stale pack refusal before a new library generation effect

Already stale selected-pack choices now refuse before the Engine records a new local-library generation effect. The original recipe stays immutable, and selection changes after this check still meet the final transactional generation fence. Existing pending request identities are preserved. Native deactivation/reselection/restart, zero added generation effects, completed cancellation and five-membership release pass; ordinary CLI/desktop publication still yields four scripts matching the diagnostic fixture. Engine tests pass 884 with 70 ignored; Clients pass 392 with ten ignored. Both strict lint configurations, Windows source check (with existing warnings), native/helper builds, formatting, architecture and 36 source guards pass. Reader floor remains 31. Full stopped-batch migration, unknown-effect disposition and model/media/platform/Handy/human/release qualification remain open; roadmap PARTIAL. See [ADR B3-177](../research/phase-b/ADR-B3-177-library-pack-preflight.md), [contract](PULSAR_LIBRARY_PACK_PREFLIGHT.md) and [checkpoint](evidence/roadmap-2026-10-12/library-pack-preflight/checkpoint.json).


## ARCH125 — explicit linked local library migration

Explicit local-library migration now creates a linked successor for unfinished entries under a checked replacement recipe, reusing captured sources and imported projects. The original batch is fenced from new generation and retains accepted jobs, completed entries and outputs. Positively acknowledged failed/cancelled jobs can obtain a fresh successor generation intent only after current terminal state and absent Worker ownership are checked; their original receipts remain unchanged. Unknown pending effects, missing positive receipts, active/interrupted work, uncommitted candidates and cancelled batches refuse. Exact report observation, current grants/revisions, fresh pack admission, immutable retry content, bounded lineage and successor-first metadata release are enforced. Reader floor is 32. Engine tests pass 888 with 70 ignored; unchanged Client/Protocol subjects retain passing 394/286 suites with ten/zero ignored. Both strict lint configurations, Windows source check (with existing warnings), native/helper builds, formatting, architecture and 36 guards pass. Native CLI/desktop migration, restart/retry, wrong/stale intent refusal, source/project reuse, predecessor refusal without database mutation, completed-prefix preservation and actual failed-Worker recovery pass. Nine published scripts match the authored detector oracle. Full roadmap PARTIAL; rendered GUI, broader faults/views/watching, unknown-effect disposition and useful model/media/platform/Handy/human/release qualification remain open. See [ADR B3-178](../research/phase-b/ADR-B3-178-library-migration.md), [contract](PULSAR_LIBRARY_MIGRATION.md) and [checkpoint](evidence/roadmap-2026-10-12/library-migration/checkpoint.json).


## ARCH126 — per-axis local companion inspection

Per-axis companion inventory now distinguishes present, missing and blocked channels through `library-axes` and the desktop background service without connecting to Engine. Existing stroke no longer hides missing secondary axes in inspection. Recognized paths and individual blockers remain visible; empty/unvalidated files count as protected presence. Six focused regressions cover partial bundles, scan limits, symlinks, local command routing and pending desktop recovery preservation. Client tests pass 400 (398 library plus two fixture tests), with ten ignored. Both strict lint configurations, Windows source check (existing warnings), native build, formatting, architecture and 36 guards pass. Native CLI inspection preserves authored bytes, creates no Engine state, reports recursive inventory/blockers and refuses invalid/oversized input. Engine/Protocol source identities and reader floor 32 are unchanged; no fresh Engine/Protocol suite or rendered GUI claim is made. Filling missing axes into existing companions/bundles remains open under TEST-15/EDIT-06; roadmap PARTIAL. See [contract](PULSAR_LIBRARY_AXIS_INVENTORY.md), [ADR B3-179](../research/phase-b/ADR-B3-179-library-axis-inventory.md) and [checkpoint](evidence/roadmap-2026-10-12/library-axis-inventory/checkpoint.json).


## ARCH127 — selected audio output axes in retained batches

Retained audio intake now captures a per-input publication subset when an adjacent regular stroke companion exists: five secondary axes publish into the recognized new audio bundle while the existing stroke bytes remain untouched. Six-channel synthesis and commit validation remain separate from output selection. Explicit audio-subset report/journal schema 5 prevents visual-family reinterpretation; cancellation, migration, restart and immutable recovery retain the selection. Reader floor is 33 and older migration admission cannot downgrade it. Engine/Clients/Protocol tests pass 889/402/287 with 70/10/0 ignored; both strict lints, Windows source check (existing warnings), native/controller builds, formatting, architecture and 36 guards pass. Final changes affect only two Client paths relative to the passing full Engine/Protocol subject. Native CLI and actual desktop controller publish 22 files matching authored PCM/timestamp oracles, preserve adjacent stroke and existing bundle bytes, repeat without extra jobs/exports, retain subset cancellation/release and prove reader-32 refusal with unchanged database bytes. Two owned Engines are joined. The adjacent-stroke case of TEST-15 has scoped software evidence; filling already-partial bundles, broader faults, real script/media/model quality, rendered GUI/platform, Handy hardware and human/release acceptance remain open. Full roadmap PARTIAL. See [contract](PULSAR_LIBRARY_SELECTED_AUDIO.md), [ADR B3-180](../research/phase-b/ADR-B3-180-library-selected-audio.md) and [checkpoint](evidence/roadmap-2026-10-12/library-selected-audio/checkpoint.json).


## ARCH128 — fill missing channels in existing local audio bundles

Existing local audio bundles now receive only absent channels through captured directory identity and no-replacement per-file publication. Present scripts, adjacent stroke, unknown members and prior receipts remain unchanged. The private journal retains exact per-channel export IDs and positive acknowledgements; matching files never substitute for missing receipts. Schema 6 and reader floor 34 preserve this shape, with no aggregate merge receipt. Engine/Clients/Protocol tests pass 891/404/288 with 70/10/0 ignored; the Engine/Protocol sources are unchanged since their passing suites. Both strict lints, Windows source checks (existing warnings), native/controller builds, formatting, architecture and 36 guards pass. Native CLI and desktop controller produce 36 files matching authored PCM/timestamp oracles across eight jobs, including private RAM and owned NVMe ext4 targets. Exact replay after injected journal loss, restart, target/stale-directory preflight refusal, missing-positive-receipt refusal and actual-positive recovery pass. Reader 33 refuses without main-database mutation; four owned Engines are joined. Initial intake and completion bookkeeping failures are retained with their repairs and reruns. This supplies scoped partial-bundle software evidence for TEST-15/EDIT-06. Broader faults, explicit unknown-effect disposition, rendered GUI/platform, useful model/media quality, Handy hardware and human/release acceptance remain open. Full roadmap PARTIAL. See [contract](PULSAR_LIBRARY_PARTIAL_BUNDLES.md), [ADR B3-181](../research/phase-b/ADR-B3-181-library-partial-bundles.md) and [checkpoint](evidence/roadmap-2026-10-12/library-partial-bundles/checkpoint.json).


## ARCH129 — direct retained-library source-view authoring

Direct visual library recipes now expose shared stereo-eye/layout and flat/360/VR180 projection controls with perspective orientation and field of view. Capture freezes the validated view in the background action and exact retry recipe; invalid declarations and conflicting JSON refuse before dispatch. The single-source editor uses the same renderer and retains seek invalidation. Clients pass 406 with ten ignored; strict lints, Windows source check (existing warnings), native/controller builds, formatting, architecture and 36 guards pass. Engine/Protocol source identities are unchanged, with no fresh suite claim. Four authored native cases produce 12 outputs with eight original timestamps, range and CLI/desktop parity; flat stereo/legacy also match a known position oracle. Spherical positions have no independent accuracy oracle. Completed CLI restart replay creates no new effects, originals are preserved and eight final Engines are joined. Initial unresolved-input, cross-session, harness cleanup and inappropriate projection-oracle failures are retained with explicit limits. Reader admission is unchanged. Full roadmap PARTIAL; rendered GUI, useful media/model/projection quality, unknown-effect disposition, platform/edition/device and human/release gates remain open. See [contract](PULSAR_LIBRARY_VIEW_CONTROLS.md), [ADR B3-182](../research/phase-b/ADR-B3-182-library-view-controls.md) and [checkpoint](evidence/roadmap-2026-10-12/library-view-controls/checkpoint.json).


## ARCH130 — selected-axis unresolved publication diagnostics

The earlier gradient-input publication refusal now has digest-bound native diagnosis: one observed detector box but zero accepted target/background flow support, explicit unavailable coverage from 125000000..875000001 ns and retained reviews. Stereo repeat and mono differential refuse; a textured control publishes. Independent gradient patch ties explain non-unique correspondence without fabricating physical labels. Shared bundle preflight now reports axis, unresolved interval count and exact first project-time gap, with empty tracks distinguished. Tracking, gap handling, schemas and reader admission are unchanged. Clients pass 406 with ten ignored; strict lints, Windows source check (existing warnings), native/controller builds, formatting, architecture and 36 guards pass. Engine/Protocol/Core source identities are unchanged, with no fresh suite claim. Final native restart retry preserves journal bytes and 4/4/1 jobs/candidates/exports; two final Engines are joined. The unresolved input remains a failed publication case, not a quality pass. Rendered GUI was unavailable because Agent Workspace MCP tools were absent; no host desktop was operated. Full roadmap PARTIAL. See [diagnosis and limits](PULSAR_UNRESOLVED_FLOW_DIAGNOSTICS.md), [ADR B3-183](../research/phase-b/ADR-B3-183-unresolved-flow-diagnostics.md) and [checkpoint](evidence/roadmap-2026-10-12/generation-unresolved-diagnosis/checkpoint.json).


## ARCH131 — acknowledgement-covered pending metadata retirement

Explicit CLI and desktop pending-journal retirement now preserves inspected bytes under a deterministic retained name only after completed current acknowledgements cover the exact history and original request IDs. No generation/export, output adoption or deletion occurs; exact retries inspect the retained file. Clients pass 409 tests with ten ignored; both strict lints, Windows source check (existing warnings), builds, formatting, architecture and 36 guards pass. Engine/Core/Protocol identities are unchanged, with no fresh suite claim. Native actual CLI death/restart/replay, genuine orphan retirement, cross-client repeats and six refusal boundaries pass; desktop positive also uses a constructed duplicate of an acknowledged journal. Seventeen outputs across four jobs match authored PCM/timing oracles, protected bytes remain unchanged and two Engines are joined. Initial fixture-permission and harness credential-assumption failures are retained. General unknown-effect disposition, power-loss/platform/rendered GUI, useful model/media quality, Handy and human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_LIBRARY_PENDING_RETIREMENT.md), [ADR B3-184](../research/phase-b/ADR-B3-184-library-pending-retirement.md) and [checkpoint](evidence/roadmap-2026-10-12/library-pending-retirement/checkpoint.json).


## ARCH132 — browser live preview and CPU preparation admission

A new local browser player feeds presentation-stamped 64×64 RGB frames into the existing authenticated live Engine and displays provisional Stroke estimates. Pause/seek and late-reply fencing, source-stale suppression, three-second input-stall stop and bounded original leases are explicit. The actual HTTP/Engine campaign exposed a one-thread staging claim transferred into a two-thread CPU worker; named preparation budgets now carry actual CPU launch limits without weakening the cgroup guard. Final native inference, Stop, fresh seek epoch, stale-epoch refusal, idle and active-producer server-exit cleanup pass against an authored scalar oracle, with empty custody and zero ordinary jobs/candidates/exports. Engine library/Client suites pass 878/410 with 70/10 ignored; strict lints, Windows source check, builds, formatting, architecture and 36 guards pass. Three mock-DOM player tests pass; rendered browser QA remains unverified. This is a local-file motion preview, not arbitrary-tab capture, funscript recording or Handy playback. Warm continuity beyond 30-second captures, useful models, measured browser/device timing, six-axis/platform/edition and human/release gates remain open. Full roadmap PARTIAL. See [contract and launch instructions](PULSAR_BROWSER_LIVE_PREVIEW.md), [ADR B3-185](../research/phase-b/ADR-B3-185-browser-live-preview.md) and [checkpoint](evidence/roadmap-2026-10-12/browser-live-preview/checkpoint.json).


## ARCH133 — automatic browser live continuation

The local browser adapter now prepares Engine successors at half-lease and keeps the browser playback identity stable across confirmed Ready/Superseded handoffs. Exact project phase derives from the existing checked clock contract; separate channels/sequences and handshake eligibility prevent relabeling old frames. Stop attempts both scopes; ambiguous outcomes fence the preview. A 50-second paced native CPU campaign observes three pending successors, three handoffs and four backend captures with preserved source/project mapping and an authored scalar oracle; Stop/seek/stale/stall/server cleanup pass with empty custody and zero ordinary jobs/candidates/exports. Clients pass 410 tests with ten ignored, four mock-player tests and strict lint/platform/build/format/architecture checks pass. Engine/Core/Protocol source identities are unchanged, with no fresh suites claimed. This removes normal manual lease restarts but does not qualify uninterrupted estimates, rendered browsers, useful models, CUDA/device timing or Handy output. Full roadmap PARTIAL. See [contract](PULSAR_BROWSER_LIVE_CONTINUATION.md), [ADR B3-186](../research/phase-b/ADR-B3-186-browser-live-continuation.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-live-continuation/checkpoint.json).


## ARCH134 — Engine-owned accepted-estimate recording

The Engine now retains accepted live inference estimates in a bounded actor-owned recording buffer with original clock/profile/attempt/pixel bindings. Stopped captures remain pageable; discard preserves a non-rearmable intent tombstone. Overflow stops inference with explicit partial data and valid counters. Native CLI/Engine recording, exact sample/time oracles, foreign/grant refusal, paging, intent/release replay, late Start and capacity failure pass with empty custody and no candidates/exports. Final Engine-library/Client/Protocol tests pass 879/410/290 with 70/10/0 ignored, and strict checks/builds pass. Initial response-Debug and capacity-counter failures are retained with repairs. Browser automatic recording, durable packages, candidate coverage/review/commit and funscript export remain unfinished; model/media/CUDA/platform/Handy/human/release qualification is open. Full roadmap PARTIAL. See [contract](PULSAR_LIVE_RECORDING_BUFFER.md), [ADR B3-187](../research/phase-b/ADR-B3-187-live-recording-buffer.md) and [checkpoint](evidence/roadmap-2026-10-13/live-recording-buffer/checkpoint.json).


## ARCH135 — browser accepted-estimate recording controls

Opt-in browser recording now begins before pixels for initial and prepared captures. The player lists retained scopes, reads accepted estimates and downloads bounded stopped-capture JSON with original evidence labels. Native checks retain 493 samples across three handoffs; actual browser download retains 152 samples in three pages. Client tests pass 410 with ten ignored, six player tests and strict checks pass. The authored scalar graph is diagnostic only. Candidate assembly, funscript export, arbitrary tabs, useful models, physical Handy and human/release qualification remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_RECORDING_CONTROLS.md), [ADR B3-188](../research/phase-b/ADR-B3-188-browser-recording-controls.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-recording-controls/checkpoint.json).


## ARCH136 — checked live-recording motion assembly

Checked [live-recording assembly](PULSAR_LIVE_RECORDING_ASSEMBLY.md) now maps accepted points to exact project time and retains explicit gaps, immutable scope/page checks and provisional CUDA presence. This is a Core/Protocol kernel with a read-only diagnostic harness; Engine candidate staging, browser review/commit/funscript export and all model/device/release qualification remain open. Full roadmap PARTIAL.


## ARCH137 — Engine-owned live recording candidates

Engine-owned recording staging now creates an uncommitted candidate with full original source proofs, inferred/provisional review flags and unavailable gaps. CLI access, original-ID replay after actual Engine restart, explicit fixture commit and unresolved-export refusal have scoped evidence. The native candidate retains 491 points and five independently checked gaps. Browser controls, gap resolution, portable live-recording clone testing and all useful-model/device/platform/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_LIVE_RECORDING_CANDIDATE.md), [ADR B3-190](../research/phase-b/ADR-B3-190-live-recording-candidate.md) and [checkpoint](evidence/roadmap-2026-10-13/live-recording-candidate/checkpoint.json).


## ARCH138 — browser recording candidate controls

Browser controls now prepare an original intent before Engine staging, display recorded candidate estimates/gaps, inspect/retry the same request and read shared diagnostics. Native HTTP tests retain 491 points through three handoffs and one candidate; actual rendered browser QA stages 253 points across two captures, retries the same candidate and displays seven issues for three gaps. Clients pass 412 with ten ignored, eight mock-player tests and strict checks pass. The initial route allow-list failure is retained with its repair/rerun. Browser commit/export, gap resolution, multi-proposal/durable server recovery and all useful-model/platform/Handy/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_RECORDING_CANDIDATES.md), [ADR B3-191](../research/phase-b/ADR-B3-191-browser-recording-candidates.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-recording-candidates/checkpoint.json).


## ARCH139 — authored selected-gap proposals

Explicit CLI gap proposals now construct selected linear bridges/nearest edge holds through shared values editing, preserve original knots/other axes/unselected gaps and reject protected or unanchored selections. Native evidence retains 492 recorded knots plus five authored knots, leaves the master unchanged before explicit commit, and exports 497 ordered neutral actions. Shared receipts independently distinguish authored gap coverage from inherited recording coverage. Core/Client tests pass 184/414 with 0/10 ignored and strict checks pass. This is a constant diagnostic fixture, not useful-model or device qualification. Browser commit/gap/export controls, broader recovery/scale/platform and real-media/Handy/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_GAP_RESOLUTION.md), [ADR B3-192](../research/phase-b/ADR-B3-192-gap-resolution.md) and [checkpoint](evidence/roadmap-2026-10-13/gap-resolution/checkpoint.json).


## ARCH140 — explicit reviewed browser commitment

Browser master replacement now requires explicit acknowledgement of a reviewed candidate/base/program digest, retains the original commit identity before dispatch, and reports acknowledged versus current revision separately. Historical recording reads retain their original revisions. Native tests retain 494 estimates across three handoffs, verify refusals and exact repeat/restart replay; rendered QA commits 239 estimates at revision one with three gaps preserved. Clients pass 415 with ten ignored, nine mock-player tests and strict checks pass. No review disposition, export or device qualification is created. Browser gap/export controls, broader recovery/concurrency/platform and useful-model/Handy/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_RECORDING_COMMIT.md), [ADR B3-193](../research/phase-b/ADR-B3-193-browser-recording-commit.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-recording-commit/checkpoint.json).


## ARCH141 — inspected browser script download

Browser controls now inspect committed Stroke, prepare a durable pinned original export intent and download actual Engine-produced bytes with exact retry. Native evidence covers 492 recorded estimates, five explicit authored gap repairs and 497 neutral actions; rendered QA independently downloads the exact two-point fixture twice. Clients pass 416 with ten ignored, ten mock-player cases and strict checks pass. Retained application export directories still need managed release/recovery. Browser gap controls, useful-model/media/platform/Handy/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_SCRIPT_DOWNLOAD.md), [ADR B3-194](../research/phase-b/ADR-B3-194-browser-script-download.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-script-download/checkpoint.json).


## ARCH142 — explicit browser gap proposals

The browser now prepares inspected committed Stroke gaps with explicit authored-motion acknowledgement, stages through shared values upload, and separately reviews/commits the repair. Confirmed staging repeats return the original candidate; unknown upload outcomes fence replacement. Rendered QA retains 79 inferred estimates plus two synthesized edge knots and downloads 81 neutral actions from revision two. Client/strict/mock controls pass. Server-loss/transfer recovery, broader concurrency/platform and useful-model/Handy/human/release qualification remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_GAP_PROPOSALS.md), [ADR B3-195](../research/phase-b/ADR-B3-195-browser-gap-proposals.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-gap-proposals/checkpoint.json).


## ARCH143 — original browser edit-upload recovery

Client edit transfer now retains original Begin/Finish identities, validates accepted-prefix status and resumes remaining bytes; finalization retries go directly through the original Engine request and durable receipt. Browser gap proposals use this path and retain separate review/commit. Targeted faults and strict/Client/mock checks pass; rendered QA exports 159 actions from 156 inferred points plus three authored repairs. Durable server-loss recovery, managed release and broader model/platform/Handy/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_GAP_RECOVERY.md), [ADR B3-196](../research/phase-b/ADR-B3-196-browser-gap-recovery.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-gap-recovery/checkpoint.json).


## ARCH144 — durable original browser gap uploads

Private session/project-bound gap journals now persist original upload identities and pre-effect phases under a workflow lock. Two actual server kills after dropped successful Engine replies recover the same lease and single gap candidate; rendered restart recovers a prepared intent without uploading, then separately reviews/commits and exports 405 diagnostic actions. Clients pass 421 with ten ignored; strict and twelve mock controls pass. Review/commit approval is not restored. Recording/unknown-commit/export recovery, managed release, broader durability/platform and useful-model/Handy/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_GAP_JOURNAL.md), [ADR B3-197](../research/phase-b/ADR-B3-197-browser-gap-journal.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-gap-journal/checkpoint.json).


## ARCH145 — original approved gap-commit recovery

Gap commit preparation now durably retains an explicitly acknowledged original request before dispatch. Current Engine readback verifies its candidate/pin; explicit replay checkpoints the original acknowledgement without changing current authority or inventing review. A native dropped successful commit reply plus abrupt server kill recovers revision two without another revision; rendered loss/restart/retry exports 534 diagnostic actions. Clients pass 421 with ten ignored; strict and thirteen mock controls pass. Recording/export recovery, managed release and broader model/platform/Handy/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_GAP_COMMIT_RECOVERY.md), [ADR B3-198](../research/phase-b/ADR-B3-198-browser-gap-commit-recovery.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-gap-commit-recovery/checkpoint.json).


## ARCH146 — durable pinned browser export recovery

Script exports now retain a private session/project-bound original registry before dispatch, checkpoint acknowledgements and byte hashes, and use validated directory-relative reads. A native lost reply/server kill recovers exactly one browser export and refuses replaced parents before Engine dispatch. Rendered loss/restart/retry returns identical 905-action diagnostic downloads. Clients pass 423 with ten ignored; strict and fourteen mock controls pass. Initial permission/lint failures are retained with explicit 0700 repair. Legacy orphan reconciliation, managed release, recording recovery and broader model/platform/Handy/human/release gates remain open; full roadmap PARTIAL. See [contract](PULSAR_BROWSER_EXPORT_RECOVERY.md), [ADR B3-199](../research/phase-b/ADR-B3-199-browser-export-recovery.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-export-recovery/checkpoint.json).


## ARCH147 — confirmed export retirement and compact history

Explicit retirement now preserves original files/receipts, refuses unknown outcomes and permits the next export on the same project. Retained history validates original side facts and bytes and replays original authenticated requests; old retirement cannot clear a newer active intent. Native and rendered sequences pass, including legacy migration and 448-action browser copies. Full roadmap PARTIAL; file reclamation and broad crash/platform/model/Handy/human/release gates remain open. See [contract](PULSAR_BROWSER_EXPORT_RETIREMENT.md) and [ADR B3-200](../research/phase-b/ADR-B3-200-browser-export-retirement.md).

## ARCH148 — empty stopped recording scopes

A rendered stop at a continuation boundary exposed a zero-estimate successor that blocked populated playback staging. Engine/Protocol now retain complete empty stopped scopes, add no points and keep all-empty groups refused. Real native staging preserves 492 points and five source scopes, including the empty final page; explicit later repairs produce 497 actions. Final Engine/Protocol/Client suites pass 882/297/423 with 70/0/10 ignored. No qualification gate is closed. See [correction](PULSAR_EMPTY_RECORDING_SCOPES.md), [ADR B3-201](../research/phase-b/ADR-B3-201-empty-recording-scopes.md) and [checkpoint](evidence/roadmap-2026-10-13/browser-export-retirement/checkpoint.json).


## ARCH149 — retained browser recording identity

The browser records original recording IDs in private session/project storage before
Begin. Restart and refresh recover inert selections under current Engine authority;
unknown reads never cause replacement recording or invented samples. A native
actual adapter kill recovers two estimates and explicitly stages the same original
recording as an uncommitted candidate with coverage gaps intact. Clients pass 424
tests with ten ignored; scoped strict/build/mock/architecture checks pass. Engine
boot-loss backups, index retirement, broader fault/platform/model/Handy/human/release
gates remain open. Full roadmap PARTIAL. See [contract](PULSAR_BROWSER_RECORDING_INDEX.md),
[ADR B3-202](../research/phase-b/ADR-B3-202-browser-recording-index.md) and
[checkpoint](evidence/roadmap-2026-10-13/browser-recording-index/checkpoint.json).


## ARCH150 — original browser recording-stage recovery

Private session/project journals now retain original recording-stage identity/options,
pre-effect attempt and confirmed candidate ID. Native successful reply loss followed
by adapter kill/restart recovers the same request and single candidate; another restart
restores no review or commit approval. Fresh diagnostics and explicit commit/repeat
preserve revision one and coverage gaps. Clients pass 425 tests with ten ignored;
strict/build/mock/architecture checks pass. Initial fixture first-frame deadline misses
and caller timing are retained; buffering evidence until Stop fixes the campaign without
relaxing production guards. Recording commit recovery, intent retirement, broader
model/media/platform/Handy/human/release gates remain open. Full roadmap PARTIAL.
See [contract](PULSAR_BROWSER_RECORDING_CANDIDATE_RECOVERY.md),
[ADR B3-203](../research/phase-b/ADR-B3-203-browser-recording-candidate-recovery.md) and
[checkpoint](evidence/roadmap-2026-10-13/browser-recording-candidate-recovery/checkpoint.json).


## ARCH151 — original approved recording-commit recovery

Recording candidate journals now retain explicitly approved commit identity/pin and
checked acknowledgement, with backwards-compatible absent approval. Native successful
reply loss plus server kill/restart replays one original commit at revision one;
changed pin/acknowledgement refuse, and acknowledged replay also survives orderly
Engine restart. Ephemeral recordings correctly become unknown after boot loss. Clients
pass 425 tests with ten ignored; strict/build/mock checks pass. An inactive cleanup
fixture now explicitly releases its own lock; production exclusion is unchanged.
Recording Begin recovery, intent retirement and broader model/media/platform/Handy/
human/release gates remain open. Full roadmap PARTIAL. See
[contract](PULSAR_BROWSER_RECORDING_COMMIT_RECOVERY.md),
[ADR B3-204](../research/phase-b/ADR-B3-204-browser-recording-commit-recovery.md) and
[checkpoint](evidence/roadmap-2026-10-13/browser-recording-commit-recovery/checkpoint.json).


## ARCH152 — confirmed recording-handoff retirement

Explicit retirement now preserves compact original stage/candidate/commit receipts
and allows another playback to stage on the same project. Unknown or unapproved
commits refuse; late old retirement never clears a new active intent. Native recovery/
retirement/new-take checks retain two candidates and unchanged committed revision one,
with empty custody and no jobs/exports. Clients pass 425 tests with ten ignored; sixteen
mock-player controls and strict/build checks pass. No Engine history or recordings
are deleted. Recording-index capacity, gap retirement and broader model/media/platform/
Handy/human/release gates remain open. Full roadmap PARTIAL. See
[contract](PULSAR_BROWSER_RECORDING_CANDIDATE_RETIREMENT.md),
[ADR B3-205](../research/phase-b/ADR-B3-205-browser-recording-candidate-retirement.md) and
[checkpoint](evidence/roadmap-2026-10-13/browser-recording-candidate-retirement/checkpoint.json).


## ARCH153 — confirmed gap-handoff retirement

Gap journals now preserve compact confirmed receipts and allow a later repair intent
after explicit retirement. Recording/gap handoffs share a receipt shape but retain
independent namespaces and authority. Native two-cycle record/commit/repair/export
checks preserve late-repeat identity and finish at revision five with four candidates,
two exports and empty custody. A protected-region refusal is retained; the final
fixture explicitly changes policy in a separate revision before replacing its own
authored repair. Production protection remains enforced. Clients pass 425 tests with
ten ignored; seventeen player mocks and strict/build checks pass. Recording-index
capacity and broader model/media/platform/Handy/human/release gates remain open. Full
roadmap PARTIAL. See [contract](PULSAR_BROWSER_GAP_RETIREMENT.md),
[ADR B3-206](../research/phase-b/ADR-B3-206-browser-gap-retirement.md) and
[checkpoint](evidence/roadmap-2026-10-13/browser-gap-retirement/checkpoint.json).


## ARCH154 — stopped playback recording-index retirement

Explicit index retirement moves entire authenticated stopped playback groups into
bounded private history, reclaiming active selections without deleting Engine buffers.
Native four-second-lease continuation produces three scopes; group retirement and all
original downloads survive adapter restart. Unavailable boot references remain unknown
and active. Clients pass 425 tests with ten ignored; eighteen player mocks and strict/
build checks pass. History release, unknown disposition, broader model/media/platform/
Handy/human/release gates remain open. Full roadmap PARTIAL. See
[contract](PULSAR_BROWSER_RECORDING_INDEX_RETIREMENT.md),
[ADR B3-207](../research/phase-b/ADR-B3-207-browser-recording-index-retirement.md) and
[checkpoint](evidence/roadmap-2026-10-13/browser-recording-index-retirement/checkpoint.json).


## ARCH155 — experimental explicit browser-tab input

A click now requests a video-only display stream, checks returned browser-surface
identity and connects it to the existing bounded frame path. Owned tracks close on
stop/source switch/revocation/exit, with late permission replies fenced. Twenty-one
player mocks, 425 Client tests with ten ignored, strict/build checks and existing native
transport regression pass. Real getDisplayMedia selection/decoding was not exercised
because isolated workspace browser tools were unavailable. Capture presentation time
is not source-player time; useful models, timing, physical Handy and platform/human/
release gates remain open. Full roadmap PARTIAL. See
[contract](PULSAR_BROWSER_TAB_CAPTURE.md),
[ADR B3-208](../research/phase-b/ADR-B3-208-browser-tab-capture.md) and
[checkpoint](evidence/roadmap-2026-10-13/browser-tab-capture/checkpoint.json).


## ARCH156 — tab-capture loss and asynchronous failure ownership

Muted/unavailable video tracks now stop sharing and clear motion. Playback rejection
cleans up the currently owned stream even after its play-event generation changed;
late permission rejection cannot match null ownership and stop a newer source. Clients
pass 425 tests with ten ignored; twenty-four player mocks and strict/build/native
transport regression checks pass. Actual browser events/stream acceptance, source
player clock, models, Handy and platform/human/release gates remain open. Full roadmap
PARTIAL. See [contract](PULSAR_BROWSER_TAB_LIFECYCLE.md),
[ADR B3-209](../research/phase-b/ADR-B3-209-browser-tab-lifecycle.md) and
[checkpoint](evidence/roadmap-2026-10-13/browser-tab-lifecycle/checkpoint.json).


## ARCH157 — declared live timestamp-domain provenance

Admitted options and recording pages now retain producer/source-media/display-capture
presentation declarations. Continuation cannot relabel them; assembly refuses mixed
domains and capture-relative candidates receive timing review diagnostics. Legacy absent
fields remain generic producer declarations. Native recording/continuation/review checks
and Engine/Protocol/Client suites pass 882/298/425 with 70/0/10 ignored; 24 player mocks
and strict/build checks pass. No source-player clock calibration, real browser capture
or physical synchronization is claimed. Full roadmap PARTIAL. See
[contract](PULSAR_LIVE_TIME_BASIS.md),
[ADR B3-210](../research/phase-b/ADR-B3-210-live-time-basis.md) and
[checkpoint](evidence/roadmap-2026-10-13/live-time-basis/checkpoint.json).


## ARCH158 — desktop ownership of the shared browser player

The Browser live panel captures the open project/revision and explicit options path, publishes the shared adapter's loopback URL, and owns cancellation and join. Stop, changed project/revision, expiry and owner destruction use existing capture cleanup; ordinary desktop close waits while rendering continues. A new launch cannot replace a still-owned server. No second generator, native credential exposure, useful default model or device output is added. See [contract](PULSAR_DESKTOP_BROWSER_LIVE.md), [ADR B3-211](../research/phase-b/ADR-B3-211-desktop-browser-launch.md) and [checkpoint](evidence/roadmap-2026-10-13/desktop-browser-launch/checkpoint.json). Full roadmap PARTIAL; actual desktop/browser/platform, source-player clocks, useful-model/media, Handy and human/release acceptance remain open.


## ARCH159 — immutable desktop-authored live options

Browser live now captures an explicit Generator file-model/profile and authors checked preview age, buffer, lease and project start without requiring an options file. Model/runtime identities are displayed; failed recapture preserves the prior choice, and launch freezes the complete typed value before worker creation. Mutable Generator/form changes cannot retarget a running server. Pack selection is not reinterpreted as raw-model admission. Engine model/runtime/provider checks and all model/media/browser/platform/device/human gates remain open. See [contract](PULSAR_DESKTOP_LIVE_OPTIONS.md), [ADR B3-212](../research/phase-b/ADR-B3-212-desktop-live-options.md) and [checkpoint](evidence/roadmap-2026-10-13/desktop-live-options/checkpoint.json). Full roadmap PARTIAL.


## ARCH160 — fresh pack-bound live admission and recording provenance

Live options can carry an exact owner-selected pack choice. Engine resolves it freshly under the artifact gate through shared integrity/profile/runtime and snapshot checks while retaining the original request ID/fingerprint. Continuation cannot erase/change the choice; registry selection loss requests cancellation. Recording/assembly/archive/durable source proof preserve its historical identity. Desktop pack capture freezes choice/profile without raw-path extraction or selection/approval effects. Final Engine/Protocol/Clients pass 882/299/434 with 70/0/11 ignored; 24 player mocks, strict/build checks and real packed CPU/continuation/withdrawal/replay/corruption/provenance tests pass. Full roadmap PARTIAL; useful models, broader faults/portability, actual rendering/platform/source-player clocks, physical devices and human/release acceptance remain open. See [contract](PULSAR_LIVE_MODEL_PACKS.md), [ADR B3-213](../research/phase-b/ADR-B3-213-live-model-packs.md) and [checkpoint](evidence/roadmap-2026-10-13/live-model-packs/checkpoint.json).


## ARCH161 — explicit browser camera input and time provenance

Camera and tab capture share one consent/lifecycle owner and the bounded RGB live
adapter. Camera presentation has a distinct retained declaration, mixed-domain
assembly refusal and capture-relative review diagnostics. No calibrated exposure
clock, useful default model or device output is inferred. See
[contract](PULSAR_BROWSER_CAMERA.md),
[ADR B3-214](../research/phase-b/ADR-B3-214-browser-camera.md) and
[checkpoint](evidence/roadmap-2026-10-14/browser-camera/checkpoint.json).
Full roadmap PARTIAL; actual camera/browser/platform, model/media, timing/device
and human/release gates remain open.


## ARCH162 — receipt-bound observations before journal I/O

The conditional Handy coordinator captures play/stop observations through an immutable
receipt binding without a database handle. Control state may be released before metadata
persistence. Play completion retains its observation independently of stop construction;
retries persist only the exact captured outcome. A binding is correspondence, not live
ownership, qualification or dispatch authority. Existing synchronous completion wrappers,
stop-record naming and journal encoding remain compatible. See
[contract](PULSAR_PLAY_COMPLETION_RECONCILIATION.md),
[ADR B5-020](../research/phase-b/ADR-B5-020-observed-play-completion.md) and
[checkpoint](evidence/roadmap-2026-10-16/play-completion/checkpoint.json).
Public physical playback and the full roadmap remain incomplete.


## ARCH163 — owned HSSP dispatch and single-use handoff claims

One dispatcher owns one HTTP job without an implicit queue/retry. Runtime claims
prevent a prepared play or stop from being handed off twice, including through
separate dispatchers. Original clock facts are rechecked after journal preparation;
original request bytes remain unchanged. A fresh Engine authorizer, exact transport
binding and current coordinator state are required before handoff. Explicit completion
feeds the receipt-bound observation path; dropping unread results leaves durable
uncertainty. Registry ownership and public qualification gates are not supplied by
this module. See [contract](PULSAR_HANDY_DISPATCH.md),
[ADR B5-021](../research/phase-b/ADR-B5-021-owned-hssp-dispatch.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-dispatch/checkpoint.json).
Full roadmap PARTIAL; physical playback remains disabled in public Engine commands.


## ARCH164 — inert owner registrations with atomic replay

Owner-scoped Handy configuration uses bounded self-versioned metadata tables and
ordinary request receipts in one Engine transaction. Historical receipts never replace
current status. Checked revisions and incarnation-bound fingerprints fence inspection;
authentication and the selected binding are rechecked after I/O. Registration resolves
no secret values and grants no networking, qualification or playback authority. The
additive reader contract applies only to inert metadata; physical activation needs its
own reviewed state/reader contract. See [contract](PULSAR_HANDY_REGISTRATIONS.md),
[ADR B5-022](../research/phase-b/ADR-B5-022-inert-handy-registrations.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-registration/checkpoint.json).
Full roadmap PARTIAL; live session/qualification/controller integration remains open.


## ARCH165 — desktop original-intent recovery and registration withdrawal

Desktop registration mutations retain exact identities before dispatch. An explicit
Engine resolution atomically returns saved history or reserves the original identity
as withdrawn; it never infers an outcome from current configuration or undoes an applied
change. Preceding readers fail closed on the unknown withdrawal response. UI recovery
restores neither mutation execution nor network consent, and positive settlement is
required before local dismissal. See [contract](PULSAR_DESKTOP_HANDY_REGISTRATIONS.md),
[ADR B5-023](../research/phase-b/ADR-B5-023-desktop-handy-intent-resolution.md) and
[checkpoint](evidence/roadmap-2026-10-16/desktop-handy-registration/checkpoint.json).
Physical playback and the full roadmap remain incomplete.

## ARCH166 — boot-bound Handy connection observations

Owner-authorized open/status/close operations retain one native credential snapshot
per registered device and connection-key route. An opening receipt commits before
inspection; exact replay returns current boot-local state or retirement without another
network call. Captured Engine epochs fence delayed requests after restart. Registration
changes invalidate observations under the same lock order as completion, and retired
openings hold their route until their bounded GET returns. Closing retires local
observation ownership and supplies no physical stopping evidence. No dispatcher is
attached or playback capability enabled by this change. See
[contract](PULSAR_HANDY_CONNECTION_SESSIONS.md),
[ADR B5-024](../research/phase-b/ADR-B5-024-boot-bound-handy-connections.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-sessions/checkpoint.json).
Full roadmap PARTIAL; qualified playback, hardware and human acceptance remain open.


## ARCH167 — exact local Stroke script preparation before hosting

Handy Stroke CSV preparation uses the existing export reservation and no-clobber
publication path. Its typed receipt binds the current master revision/hash, output
identity, quantization errors and review state. The ledger keeps the actual Stroke axis,
so portable project capture retains inert export metadata without loading a mutable
published path. Historical receipt replay does not re-create or overwrite the file.
Neutral samples do not inherit the analytic adapter's quintic dynamics bounds, cloud
sharing consent or device authority. See [contract](PULSAR_HANDY_SCRIPT_PREPARATION.md),
[ADR B5-025](../research/phase-b/ADR-B5-025-exact-stroke-script-preparation.md), and
[checkpoint](evidence/roadmap-2026-10-16/handy-script/checkpoint.json).
Full roadmap PARTIAL; hosting/setup and qualified synchronized playback remain open.


## ARCH168 — archival proposal scope and package format v14

Portable export history distinguishes a selected Core axis from a whole analytic
DeviceMotion proposal. The legacy axis wire field remains byte-compatible for ordinary
receipts; the composite tag requires package format v14 and survives retained origins.
All publication states remain historical, external paths are never resolved, and neutral
motion verification requires a matching single-axis scope. See
[contract](PULSAR_DEVICE_EXPORT_ARCHIVES.md),
[ADR B5-026](../research/phase-b/ADR-B5-026-device-export-archival-scope.md), and
[checkpoint](evidence/roadmap-2026-10-16/device-export-archive/checkpoint.json).
This repairs the reproduced package failure without adding device or human authority.
Full roadmap PARTIAL; hosting/setup, physical and release gates remain open.


## ARCH169 — explicit exact-script hosting and durable disclosure uncertainty

Temporary public hosting requires original preparation identity, exact bytes, current
project revision, owner capability, Read/Export grants and explicit disclosure consent.
An unknown-outcome receipt commits before network I/O; exact replay never sends again.
A constrained same-origin download must match every authorized CSV row before verification.
Only observations persist. Native locations are retired by explicit forgetting, restart
or stale completion, and cannot grant HSSP setup or physical playback authority. See
[contract](PULSAR_HANDY_HOSTING.md),
[ADR B5-027](../research/phase-b/ADR-B5-027-explicit-handy-hosting.md), and
[checkpoint](evidence/roadmap-2026-10-16/handy-hosting/checkpoint.json).
Full roadmap PARTIAL; real provider, loaded-device/controller and physical acceptance remain open.


## ARCH170 — fresh read-only setup observations across retained references

The Engine captures the current native device credentials and hosting location, brackets
an exact hosted-byte recheck with device observations, and rechecks current authorization,
revision, registration and both references before returning. Selected mode/HSSP fields
remain remote observations; they neither associate the current stream with the desired
script nor prove physical stopping. The report is unqualified, records no setup and cannot
be replayed as admission authority. See
[contract](PULSAR_HANDY_SETUP_INSPECTION.md),
[ADR B5-028](../research/phase-b/ADR-B5-028-fresh-handy-setup-observations.md), and
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-inspection/checkpoint.json).
Full roadmap PARTIAL; qualified setup, dispatch and actual device/release acceptance remain open.


## ARCH171 — retained qualification evidence and explicit operator review

Bind bounded retained evidence to the registration, observed device identity, loaded control
image, platform, transport and declared native HSSP profile. Import and its receipt commit
atomically without acceptance. Separate owner-authorized review rechecks evidence and fresh
identity; revision checks prevent late completion from undoing withdrawal. Synthetic origin
cannot receive physical acceptance. Withdrawal remains possible with corrupt payloads.
Selected accepted records are checked again after setup-inspection I/O; a matching record
is one admission input and does not set up a script or prove physical stopping. See
[ADR B5-029](../research/phase-b/ADR-B5-029-bound-handy-qualification-review.md) and
[contract](PULSAR_HANDY_QUALIFICATION.md). Validation is recorded in the
[checkpoint](evidence/roadmap-2026-10-16/handy-qualification/checkpoint.json).
Full roadmap PARTIAL; actual device, platform, media, model and human acceptance remain open.

ARCH171 follow-up: a native hosting reference now retains both source/download identities
and the extrema of their verified quantized rows. Selecting an accepted dossier checks
position range, every adjacent step and minimum interval before and after inspection. This
applies discrete limits without exposing a grant or claiming an interpolation/physical proof.
The [sample-envelope checkpoint](evidence/roadmap-2026-10-16/handy-sample-envelope/checkpoint.json)
records scoped validation. No new persistent or wire format is introduced.


## ARCH172 — explicit operative settings in versioned qualification subjects

Version-2 subjects bind complete declared slider settings and stroke limits. Version-1
identities and history remain readable but cannot acquire settings-aware authority through
defaults. Acceptance requires consistent fresh observations; setup matching compares the
reviewed settings and rechecks the accepted record after I/O. Native timing configuration
includes those settings. No settings write, setup, playback or physical qualification is
implied. See [ADR B5-030](../research/phase-b/ADR-B5-030-operative-settings-qualification-subject.md),
[contract](PULSAR_HANDY_OPERATIVE_SETTINGS.md), and
[checkpoint](evidence/roadmap-2026-10-16/handy-operative-settings/checkpoint.json).
Full roadmap PARTIAL; physical dynamics, setup/controller/clock and independent gates remain open.


## ARCH173 — durable setup claims and read-only journal refusal

Journal version 2 separates prepared setup metadata from its single durable claim, retains
uncertainty and reserves routes across boots. Actor/request identity cannot be rebound to
another payload, hosting request or route, even after cancellation. Only provably unclaimed
preparation can be cancelled by the current owner; history restores no claim. Legacy play/stop
rows are preserved during upgrade. A read-only preflight prevents the current reader from
checkpointing an unsupported journal WAL while refusing it. The preceding reader's observed
WAL change is retained separately from logical-row preservation. See
[ADR B5-031](../research/phase-b/ADR-B5-031-single-use-setup-journal.md),
[contract](PULSAR_HANDY_SETUP_JOURNAL.md), and
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-journal/checkpoint.json).
This is a native backend step; public setup transport, route/authority acquisition, loaded
program correlation, reconciliation and actual device qualification remain open.


## ARCH174 — actor/request setup recovery and private route namespaces

Expose read-only setup history to its original actor under current owner and project Read
authority. Recover by request ID without receipt IDs, credentials or source files. Derive
credential routes with HMAC under the existing private owner capability and pin the key
namespace in new records. Changed or absent active namespaces block preparation; logical
device reservations also prevent a new key from bypassing outstanding history. Old records
remain readable and preceding readers reject new metadata before writable startup. See
[ADR B5-032](../research/phase-b/ADR-B5-032-private-routes-and-setup-recovery.md),
[contract](PULSAR_HANDY_SETUP_RECOVERY.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-recovery/checkpoint.json).
Actual setup sending, authority acquisition at handoff, loaded-program correlation and
reconciliation remain integration work; full roadmap and physical qualification remain open.


## ARCH175 — explicit qualified setup sender and cross-store record links

The shared protocol and CLI can send one qualified setup after fresh script/device/settings
preflight and explicit consent. Global request reservation, setup preparation and a durable
project-to-journal record link precede the single-use claim. The fixed-origin PUT runs outside
metadata locks; retries return history rather than redispatching. Startup refuses missing
linked records before recreating storage. Acknowledgment is historical transport data only,
with loaded/qualified flags still false. See
[ADR B5-033](../research/phase-b/ADR-B5-033-explicit-qualified-setup-handoff.md),
[contract](PULSAR_HANDY_SETUP_SEND.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-send/checkpoint.json).
Loaded-program correlation, reconciliation, mode/control lifecycle, synchronized playback
and actual model/media/platform/device/human/release qualification remain open.

## ARCH176 — exact unclaimed setup cancellation with separate recovery receipts

Cancellation pins the original actor/request and record identity under current owner and
project Read authority. Reserve a distinct pending cancellation receipt before changing the
journal, then settle it after cancellation. Exact retry can complete metadata after a lost
reply without touching a replacement preparation. A separate response kind prevents these
receipts from being mistaken for original setup witnesses. Claimed, acknowledged and uncertain
attempts cannot be cancelled locally. See
[ADR B5-034](../research/phase-b/ADR-B5-034-exact-unclaimed-setup-cancellation.md),
[contract](PULSAR_HANDY_SETUP_CANCELLATION.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-cancel/checkpoint.json).
The 2026-10-21 scoped software campaign passes, including native lost-reply/restart recovery.
Loaded-program correlation, attempted-effect reconciliation, controller/clock integration
and physical qualification remain open.

## ARCH177 — atomic retained setup observations with explicit legacy absence

Journal version 3 stores sanitized acknowledgment data separately from immutable setup intent
records and commits both response and terminal state atomically. New intent metadata requires
the response on acknowledgment, preventing missing data from masquerading as a legacy record.
Bounded checksums include the original setup identity; exact retries cannot replace observations.
Legacy intent bytes remain unchanged and missing legacy response fields stay absent. History
does not restore loaded-program, controller or playback authority. See
[ADR B5-035](../research/phase-b/ADR-B5-035-retained-setup-observations.md),
[contract](PULSAR_HANDY_SETUP_OBSERVATIONS.md) and
[checkpoint](evidence/roadmap-2026-10-21/handy-setup-observation/checkpoint.json).
The scoped 2026-10-21 campaign passes; preceding-reader refusal preserves database/WAL bytes
while changing SQLite shared-memory sidecars. Fresh correlation and the remaining
playback/qualification gates remain open.

## ARCH178 — fresh stopped-stream association with streaming buffer semantics

New setup intents retain the observed mode generation. The explicit read-only program
inspection pins the original acknowledged setup, current project/connection/hosting and
qualification, then checks complete stopped HSP observations around a hosted-byte recheck.
Current authority is checked again after I/O. Script shape and both buffer samples remain
separate: HSSP can fill a buffer over time, so a one-buffer file limit or undocumented tail
index origin would be incorrect. The response grants no remote-content proof or playback
authority. See [ADR B5-036](../research/phase-b/ADR-B5-036-fresh-stream-correlation.md),
[guide](PULSAR_HANDY_PROGRAM_CORRELATION.md) and
[checkpoint](evidence/roadmap-2026-10-21/handy-program-correlation/checkpoint.json).
The 2026-10-22 scoped campaign passes, including native refusal/restart and preceding-reader
checks. Native playback authority, controller/clock integration and independent
device/model/media/platform/human/release gates remain open.

## ARCH179 — boot-local nonplaying preparation with durable recovery and retirement

Retain the verified program, accepted timing, captured credentials, disarmed runtime and exact
transport binding privately after committing its request receipt. A missing native entry is
retired and cannot be reconstructed by replay or restart. Project/grant and dependent device,
connection, qualification and hosting changes retire preparations before replying. An existence
query over matching play/stop history preserves physical uncertainty without interpreting a
bounded page or stopped-stream response as physical evidence. Ordinary IPC authentication does
not establish controller liveness. See
[ADR B5-037](../research/phase-b/ADR-B5-037-boot-local-nonplaying-preparation.md),
[guide](PULSAR_HANDY_PLAYBACK_PREPARATION.md) and
[checkpoint](evidence/roadmap-2026-10-22/handy-playback-preparation/checkpoint.json).
The scoped 2026-10-22 campaign passes, including immediate retirement boundaries and native
lost-reply/restart recovery. Live controller and clock authority, physical dispatch/reconciliation,
browser synchronization and independent acceptance gates remain open.


## ARCH180 — renewable local controller leases with independent expiry

A distinct Engine-issued controller lease can arm a current native preparation locally after
explicit consent and fresh program checks. Generation-checked renewals cannot revive expired
state, and exact replay cannot extend deadlines. Linux CLOCK_BOOTTIME supplies suspend-inclusive
host deadlines. An owned wakeable worker holds no Engine or database reference, expires under
DB contention and joins before other Engine resources retire. Configuration retirement also
revokes local arming. Unexpected physical uncertainty and stop intent remain retained.
See [ADR B5-038](../research/phase-b/ADR-B5-038-renewable-local-controller-leases.md),
[guide](PULSAR_HANDY_CONTROLLER_LEASES.md) and
[checkpoint](evidence/roadmap-2026-10-22/handy-controller-leases/checkpoint.json).
The scoped campaign passes. Lease duration is host authorization policy, not a physical
stopping bound. Physical dispatch, timing/synchronization and independent acceptance gates
remain open; full roadmap PARTIAL.

## ARCH181 — explicit server-clock contract and native observation custody

Qualification subject version 3 binds reviewed server timestamp origin/error assumptions;
older subjects receive no inferred timing authority. Native acquisition covers the full request
bracket, error, drift and grid rounding, with freshness measured from transmission. Each sample
belongs to one controller generation and attempt. Durable observations do not restore native
state; exact retry never resamples. Publication rechecks current authority and expiry after I/O
and receipt commit. See [ADR B5-039](../research/phase-b/ADR-B5-039-native-server-clock-contract-and-custody.md)
and the [server-clock guide](PULSAR_HANDY_SERVER_CLOCK.md). Device HSTP, browser correspondence,
physical dispatch and independent acceptance remain open.

## ARCH182 — device timing observations preserve unknown synchronization

Read-only controller-scoped inspection captures two device time/user-offset pairs between
device context observations. Preserve signed integer offsets, following explicit vendor examples
despite the offset schema's contradictory nonnegative reference. Refuse fractional/inexact
values. Report no inferred synchronization age, accuracy or authority; the API supplies none.
Context, current references and exact controller generation are rechecked after I/O, and failed
inspection cannot retire a later generation. Keep clocksync outside the read-only broker.
See [ADR B5-040](../research/phase-b/ADR-B5-040-device-timing-observations-without-inferred-synchronization.md)
and the [device timing guide](PULSAR_HANDY_DEVICE_TIMING.md). Physical dispatch remains disabled.

## ARCH183 — independent synchronization custody and single-use effect claims

Explicit synchronization uses its own version-4 device-journal records, attached to the exact
acknowledged setup and controller selection. Main request reservation, journal intent, witness
and claim commit before one effectful GET. Controller and server-clock authority retire before
handoff. Unknown custody survives native expiry/restart and blocks new preparation matching
either the credential route or logical device. Only unclaimed intent can be cancelled; exact retry reads
history. Acknowledgment permits fresh preparation but grants no timing accuracy or playback.
See [ADR B5-041](../research/phase-b/ADR-B5-041-independent-clock-synchronization-custody.md),
[guide](PULSAR_HANDY_CLOCK_SYNC.md) and
[checkpoint](evidence/roadmap-2026-10-22/handy-clock-sync/checkpoint.json).
Attempted synchronization reconciliation, native device/combined timing qualification and
physical dispatch remain open.

## ARCH184 — native synchronization lifetime and combined playback-clock expiry

Version-4 qualification declares device error, age, user offset, RTT applicability and drift.
Reserve a native context before synchronization handoff; publish completion only after its
journal commit and unchanged dependency checks. Retain it independently of the consumed
controller, including pending-context retirement so revoke/regrant cannot revive a callback.
A fresh controller explicitly selects that synchronization and rechecks device observations
before acquiring a server sample. Add both uncertainties against one budget and preserve the
earlier absolute source expiry through runtime reanchoring. Saved reports restore no authority.
See [ADR B5-042](../research/phase-b/ADR-B5-042-native-device-timing-and-combined-expiry.md) and the
[combined timing guide](PULSAR_HANDY_PLAYBACK_CLOCK.md). Physical dispatch and actual qualification
remain open.


## ARCH185 — independent retained-stop ownership and guarded dispatch claims

Share the prepared runtime and captured transport with an Engine-owned stop/completion worker
that holds registry/journal owners without retaining Engine. Keep expiry independent of metadata
and network work. Capture a boot-bound current-effect intent under control ownership, record it
outside that lock, and recheck the exact effect before handoff. Retain rejected requests and
failed completion metadata; repeated retirement cannot generate automatic physical retries.
The dispatcher supplies an invocation-bound claim continuation so fresh authorization can retain
its dependency-mutation guard through the final claim, releasing it before HTTP. See
[ADR B5-043](../research/phase-b/ADR-B5-043-independent-stop-owner-and-guarded-claims.md) and the
[stop owner guide](PULSAR_HANDY_STOP_OWNER.md). Native play submission/authority, graceful daemon
shutdown, emergency stopping, uncertain-effect reconciliation and actual qualification remain
open. Physical playback remains disabled.


## ARCH186 — original play-operation identity and durable claim boundary

Device-journal version 5 atomically records an original play operation and its sealed effect,
including actor/request fingerprint, acknowledged setup, derived preparation, controller,
qualification and original clock metadata. A durable single-use claim releases the dispatch
receipt; operation/effect outcomes commit together. Only an exact unclaimed operation can be
cancelled as unsent. Legacy effects cannot be adopted into that stronger unclaimed state.
History never restores native authority, and prior journal readers refuse v5. See
[ADR B5-044](../research/phase-b/ADR-B5-044-original-play-operation-journal.md) and the
[play-intent guide](PULSAR_HANDY_PLAY_INTENTS.md). Main-request witness/queue integration, current
play authority and public playback remain unfinished; physical playback remains disabled.

## ARCH187 — guarded native play submission and shared execution ownership

Reserve the original play request before local effect construction, record the typed journal
operation/effect and exact main witness before queueing, and recover missing metadata without
recreating native work. A standalone final authorizer shares the original DB mutex and retains
dependency guards through the runtime claim. One owner dispatches play and stop against the same
runtime and transport; failed play metadata does not block stop progression. Playing controller
renewal preserves original synchronization age. Journal v6 prevents earlier readers from bypassing
play-route custody through setup or synchronization. See
[ADR B5-045](../research/phase-b/ADR-B5-045-guarded-native-play-submission.md) and the
[submission guide](PULSAR_HANDY_PLAY_SUBMISSION.md). Scoped CLI commands are available behind
current admission checks; generic desktop playback remains disabled. Browser correspondence,
continuous clock renewal, attempted-effect reconciliation, daemon shutdown, emergency stopping
and actual qualification remain open.


## ARCH188 — owned daemon shutdown and joined background work

SIGTERM/SIGINT set a shared closing flag; ordinary threads reject new work, retire Handy
controllers, cancel operation registries, interrupt IPC and join supervisors. Both listeners
retain connection handles and descriptors; socket cleanup checks identity. A bounded task group
retains background work with outside-lock joins and panic diagnostics, while native preparations
retain their existing producer handles. Check worker-fence custody before releasing the instance
lock. Startup failure uses the same lifecycle. See
[ADR B5-046](../research/phase-b/ADR-B5-046-owned-daemon-shutdown.md) and the
[shutdown guide](PULSAR_ENGINE_SHUTDOWN.md). Linux native signal/IPC tests and synthetic localhost
Handy tests establish their declared software scope. Physical stop proof, emergency stopping
under storage failure, browser correspondence and actual platform/device qualification remain open.


## ARCH189 — emergency stop custody without a confirmed prewrite

A current sealed stop may use the same dispatcher when persistence cannot confirm its prewrite.
Keep that envelope separate from play and durable receipts; share the ordinary stop claim sequence
and captured transport checks. Known foreign journal ownership still refuses. Retain attempted
outcomes for metadata-only retry, and report unresolved stop dispatch/metadata at shutdown.
Journal v7 records emergency provenance and a bounded census atomically with exact terminal stop
history. The read-only status command identifies an original controller and always discloses
incomplete history. See [ADR B5-047](../research/phase-b/ADR-B5-047-emergency-stop-without-confirmed-prewrite.md)
and the [emergency-stop guide](PULSAR_HANDY_EMERGENCY_STOP.md). Localhost and synthetic native
history evidence do not establish physical stopping or emergency qualification.

## ARCH190 — exact custody resolution with fresh qualified observations

Keep original commands and outcomes immutable. A separate resolution records the exact custody
members and fresh observations collected after the selected device and route aliases have drained.
Qualification subject v5 declares prior-command settling, stopped-sample separation and observation
span bounds; the declaration needs actual physical evidence and current acceptance before use.
Existing subjects remain readable and cannot acquire this new authority implicitly. Compare old
physical subjects with the current accepted identity/settings so route equality cannot resolve a
different device's uncertainty.

Keep local rejection records until exact durable resolution membership permits cleanup, and do
metadata I/O outside registry/runtime guards. Historical replay may repair status only. Fresh
explicit setup, preparation, controller and timing operations are required for resumption. See
[ADR B5-048](../research/phase-b/ADR-B5-048-qualified-custody-reconciliation.md).
The implementation has combined Engine and synthetic native CLI evidence, including unrelated
renewal, exact cleanup, restart and reader/corruption refusal. See the
[recovery guide](PULSAR_HANDY_RECONCILIATION.md). This establishes no physical qualification,
browser synchronization or release acceptance.


## ARCH191 — independently observed timing during Playing

Preserve the original play reference, initial uncertainty and synchronization acknowledgment.
A separately qualified native observation may sustain an already acknowledged Playing stream
only while its preceding authority remains valid. Measure reported playtime and independent
server-time intervals; do not derive execution time from uptime plus offset or issue clock/media
time corrections. Subject v6 declares exact physical readback-error, span, age and drift bounds.

Controller renewal preserves observation timestamps and expiry. Generation/attempt fences prevent
stale completion from changing newer state; publication follows persistence and a current native
check. Original synchronization may survive as lineage only while an independently valid slot
references it. Explicit removal invalidates that slot. Stop ownership remains independent of
network reads; status/replay never resamples or restores authority. See
[ADR B5-049](../research/phase-b/ADR-B5-049-continuous-playing-timing.md) and the
[feature/evidence guide](PULSAR_HANDY_CONTINUOUS_TIMING.md). Browser presentation binding and HSP
point streaming remain distinct required integrations; software tests establish no physical timing.


## ARCH192 — independent browser liveness and exact native stop guards

Separate bounded HTTP parsing/control from the sole inference/editor worker. Bind every page,
player, source selection, queued job and result to exact scope; page GET and stale lifecycle
messages cannot replace a current player. Native binding uses current acknowledged play and
normal authorization. Subsequent nonce-bound liveness/release uses only that exact boot-local
cached principal and owner proof, before DB-dependent dispatch. Public revocation and native
retirement converge before their operations return.

Challenge issue anchors heartbeat freshness; independent frame/media progress, controller lease
and device timing must all remain valid. The existing monitor and stop owner retire expired guards.
A guard cannot authorize play, calibrate browser presentation or restore authority through history.
Deduplicate releases and retain sanitized unknown outcomes through joined shutdown; process-local
diagnostics are not durable physical-effect recovery. See [ADR B5-050](../research/phase-b/ADR-B5-050-browser-player-stop-guard.md)
and the [implementation/evidence guide](PULSAR_BROWSER_PLAYER_CONTROL.md). Source correspondence,
synchronized start and generated HSP point streaming remain required.

## ARCH193 — verified browser source reads and declared clock correspondence

Prepare exact imported bytes under current actor/project/revision and active-placement
authority before exposing a bounded same-origin media URL. Full-file verification builds
a resource-admitted block index; each returned range authenticates its affected blocks.
Retained native pins and explicit retirement protect read/capture/optional stop-guard custody.
Historical metadata cannot reconstruct the reader or revive authority.

Compose explicitly declared browser/elapsed-source anchors with the exact placement.
Capture starts at its first eligible frame and preserves mapped project phase and leading
gaps across continuation, recordings and candidate provenance. Preparation is distinct from
Play, and ordinary pause/seek need not recreate the read lease. Exact successor/predecessor
status reconciliation resolves readiness races without rearming retired captures; queued
programmatic pause events cannot affect a new explicit playback.

Unix CLI termination requests normal joined cleanup using scoped signal registrations.
The descriptor reader is currently Unix-only; byte identity is not RGB-origin attestation,
clock calibration, useful-model qualification or synchronized device playback. See
[ADR B5-051](../research/phase-b/ADR-B5-051-browser-source-media-binding.md) and
[implementation/evidence](PULSAR_BROWSER_SOURCE_MEDIA.md). Calibrated presentation,
synchronized start, periodic timing maintenance and generated HSP streaming remain required.

## ARCH194 — source-bound browser execution brackets before play

Give browser timing its own native lifetime before Handy play exists. An authenticated Open
pins exact prepared media, player scope and declared project phase; its durable original report
cannot reopen that lifetime after retirement or restart. A fresh post-probe frame callback
reports callback/sample/media timestamps together. The native owner retains the full BOOTTIME
issue-to-ingress bracket, with no half-roundtrip or physical-display assumption.

Conditional timestamp mapping uses explicit error/rate/age premises, reciprocal interval
bounds and outward rounding. Equal reported ticks do not identify one event and retain both
timestamp errors. Native expiry begins at issue; stale nonces cannot retire successors. The
independent control path, source retirement and joined shutdown preserve ownership while
inference or the database is busy. Read-only paused-player history cannot restart measurement.
Confirmed client records are reclaimed only after their worker and reply custody settle.

See [ADR B5-052](../research/phase-b/ADR-B5-052-browser-frame-clock-observations.md) and
[implementation/evidence](PULSAR_BROWSER_TIMING.md). Software, native HTTP and rendered
authored-fixture checks pass. Display/source calibration, browser-observed synchronized start,
generated HSP streaming and actual model/device/release qualification remain separate obligations.

## ARCH195 — immutable browser-observed start with continuing native ownership

Keep browser/source timing qualification separate from the existing exact accepted device-v6
contract. Its reviewed environment, display/source bounds and prepared-script identity are
required inputs; successful callback exchanges and operator environment assertions alone cannot
create those measurements. Compute the complete phase interval and server-time reference at one
native instant, retain all error components once, and preserve both wire values through delay.

Reserve cancellation for the original browser Start before database-dependent admission. Stable
player/source lifetime survives new observations and same-controller renewal, while the original
decision and expiry stay immutable. Independently fresh browser/device observations may sustain
acknowledged playback only through overlapping valid authority. The existing effect sender and
stop owner retain custody through cancellation, blocked replies, unknown outcomes and shutdown.

Journal v9 binds browser decisions to original play membership and fences older readers without
rewriting legacy manual records. Native pins, rather than deserialized history, control current
eligibility. Inactive reports retain the original decision even after retirement. See
[ADR B5-053](../research/phase-b/ADR-B5-053-browser-observed-handy-start.md) and the
[implementation and verification guide](PULSAR_BROWSER_HANDY_START.md). Generated HSP delivery,
simple controller reacquisition, actual measurements and full release acceptance remain required.

## ARCH196 — bounded generated HSP point streams

Give live generated points a distinct stream identity and append lifecycle;
completed hosted-script identity cannot describe an evolving point sequence.
Keep canonical 0–100 percent positions, signed phase/status fields, cumulative
indices and a bounded queue with one immutable append in flight. Independently
bounded phase and source coverage govern retention and eligibility; approximate
remote buffer status cannot provide point-content or physical-motion proof.

Require explicit Play and retain its original phase/server-time pair and expiry.
An unknown append, coverage gap or expired timing retires further motion and
preserves stop custody. Settling a transport task is distinct from requesting
cancellation; a stopped local task does not prove physical stopping. Pre-Play
observed options need not equal the settings that Play will explicitly request.

See [ADR B5-054](../research/phase-b/ADR-B5-054-bounded-generated-handy-streams.md)
and [component scope and remaining integration](PULSAR_HANDY_GENERATED_STREAMS.md).
Durable single-use claims, shared device ownership, qualification/admission,
native dispatch, accepted-inference delivery and application controls must be
composed before enabling the product path. All actual acceptance gates remain.

## ARCH197 — durable HSP command custody and bounded native dispatch

Keep HSP stream and command history in journal v10 with a preceding-reader
barrier, exact original membership and single-use durable claims. Shared
device/credential-route exclusion covers hosted-script admission and legacy
reconciliation; restart and deserialized history never recover live authority.
Unknown delivery remains unknown, and Stop retains separate reserved metadata
capacity and physical custody after its transport acknowledgment.

Bind native credentials and observed device/session/mode to the original record
at the final claim, then perform bounded I/O outside control/database locks.
Cancel and join the transport before freeing its operation slot. A preclaim
rejection needs an exact journal cancellation proof to retire as known-unsent;
another worker's claimed clone cannot be abandoned. Metadata retry cannot resend.

See [ADR B5-055](../research/phase-b/ADR-B5-055-durable-hsp-command-custody.md) and
[implementation scope](PULSAR_HANDY_HSP_DISPATCH.md). Public admission, continuous
ownership/emergency stopping, HSP reconciliation, useful live motion and actual
device/platform/human/release acceptance remain required.

## ARCH198 — native HSP Stop reservation during unavailable metadata

Durable and emergency HSP dispatch share an exact runtime reservation and one
worker slot. Acquire the reservation before starting work; only the joined worker
can release a confirmed unclaimed attempt or settle a consumed claim. Cancellation,
a cloned effect and a fabricated rejection cannot free another worker's custody.
Join does not acquire runtime or journal locks.

Permit only an already-issued Stop to use the retained native transport without
a confirmed journal write. Preserve device/session/mode/stream preflight and refuse
known owner replacement. Keep the resulting retrospective observation distinct
from durable history and freeze the original Unknown judgment. It cannot resend,
clear physical custody or restore playback. Retrospective persistence, ordinary-slot
retirement during an outage and continuous Engine ownership remain integration work.

See [ADR B5-056](../research/phase-b/ADR-B5-056-native-hsp-stop-reservation.md) and
[implementation scope](PULSAR_HANDY_HSP_EMERGENCY_STOP.md). The product stream path
and all actual acceptance gates remain incomplete.

## ARCH199 — Engine-owned HSP lifecycle and serialized evaluation clocks

Own each retained HSP runtime, native dispatcher, post-Setup acquisition and joined
completion under one Engine-lifetime pump. Bound admission and retained sessions;
feed the existing point queue directly. Retire exact native-unsent ordinary work
independently of database cancellation so an unavailable writer cannot indefinitely
occupy the slot needed for Stop. Retain exact runtime and transport identities
through post-Setup rebinding and preserve original command uncertainty.

Keep current authority checks independently live during metadata and runtime-lock
contention. Read evaluation time and facts under the runtime mutation guard, while
preserving original sample and pending Play timestamps. An earlier independent
expiry observation must not become a later runtime mutation's clock. Close admission
before shutdown, retire ordinary authority and join all owned work. Final joins may
exceed the bounded drain interval; no physical Stop latency is inferred.

Keep journal reads and durable claim commits outside the runtime critical section.
Exact native reservation custody prevents unsent retirement or a replacement writer
during I/O. After commit, release the journal lock and recheck fresh facts, current
runtime state and the original effect under a newly acquired runtime guard. A
failed final check is an Unknown claimed attempt and does not send or renew expiry.

Expose only authenticated read-only retained-session status until genuine HSP
qualification and accepted-inference admission exist. Reads neither create a pump
nor renew authority. Native unsent retirement, durable cancellation, transport
acknowledgment and physical custody resolution remain distinct.

See [ADR B5-057](../research/phase-b/ADR-B5-057-engine-owned-hsp-session.md) and
[implementation scope](PULSAR_HANDY_HSP_OWNER.md). Clock-domain bridging, public
admission, browser control, emergency history and actual acceptance remain open.


## ARCH200 — HSP qualification and exact native recovery

Keep generated HSP qualification separate from hosted HSSP while reusing the
existing evidence store, operator decisions and recovery workflow. Subject v7
binds the supported HSP options/policy and common physical/settings/clock envelope.
Old subject versions and legacy empty-HSP history retain their serialized identity.

Journal v11 stores immutable original qualification links and separate retrospective
emergency observations. Preserve original Unknown outcomes and complete exact
stream/effect/emergency membership. Missing historic links stay unqualified.
Do not turn emergency metadata into a durable ordinary claim or a replay handle.

All pump journal I/O uses owned bounded metadata workers. Retry mutex contention
with the same task/token and one fixed episode; retain independent original
source/effect expiry and native Stop. Join workers instead of detaching storage.

Native recovery capture fences matching admission; exact drain seals joined
memberships. Fresh transport-specific samples, current acceptance/scope and history
are checked before immutable resolution. Only its verified receipt can release the
matching native proof. Failure/replay never reopens old sessions; unrelated streams
remain usable. See [ADR B5-058](../research/phase-b/ADR-B5-058-hsp-qualification-and-custody-recovery.md)
and [the implementation guide](PULSAR_HANDY_HSP_RECOVERY.md). Native preparation,
accepted inference, browser control and actual qualification remain open.


## ARCH201 — immutable native capture clock correspondence

Map the existing capture-relative clock through one immutable native observation
bracket per epoch. Preserve both bounds and original capture/acceptance timestamps.
A changed clock relationship retires the epoch; cleanup uses the last valid tick.
Initial native-clock unavailability preserves preview but cannot be repaired by
rebasing an existing epoch into device timing authority.

Retain only accepted CPU results with exact original pair identity and continuity
provenance. Pending replacement cannot rewrite issued work; source discontinuity,
staleness, failure and retirement prevent usable native acceptance. Provisional CUDA
output remains excluded. This metadata does not qualify acquisition, model output,
physical motion or HSP dispatch. See [ADR B5-059](../research/phase-b/ADR-B5-059-native-capture-clock-correspondence.md)
and [the guide](PULSAR_LIVE_CAPTURE_CLOCK.md).


## ARCH202 — original HSP preparation and native clock handoff

Own HSP synchronization independently of hosted controller/program preparation.
Reserve the original request before work and capture its budget before locks or
storage. Fresh native inspection, current HSP acceptance and one durable claim
precede the effectful clocksync GET. Original replay may finish metadata; it cannot
resend synchronization, renew its deadline or reconstruct native clocks.

Build readiness from separately acquired device and server observations, keeping
playback disabled. A private source handoff consumes it once and verifies the exact
clock domain, original server reference, full uncertainty and expiry before and
after durable claim I/O. Transport retains original validity and retirement checks.
A source timestamp cannot substitute for those retained native facts.

Journal v12 seals the exact preparation-to-child link and transfers route custody
only atomically with the first child Setup claim. Unclaimed cancellation, attempted
synchronization, a cancelled child and qualified custody resolution remain distinct.
Shared exclusion and recovery include original preparations and exact linked child
history. Preserve older bytes and Unknown outcomes; restart restores no readiness.
See [ADR B5-060](../research/phase-b/ADR-B5-060-hsp-preparation.md) and
[the public preparation guide](PULSAR_HANDY_HSP_PREPARATION.md). Production source
admission, public stream/browser controls and actual qualification remain open.


## ARCH203 — native browser analysis feed and explicit live HSP control

Use an independent analysis reader for actual future frames and the visible
player's native timing for presentation phase. Bind one exact source, placement,
declared clock, CPU profile, environment, lookahead and age contract to a separate
reviewed source dossier and current physical schema7 HSP acceptance. Keep the
capture's HTTP receipt bracket distinct from acquisition; include the entire
accepted capture-to-receipt bound in conservative sample age.

Deliver accepted native output directly to the retained HSP controller after
producer locks are released. Freeze zero at the first actual accepted sample and
preserve its position. Quantize and check every subsequent point against the
accepted discrete envelope and source continuity without inventing leading holds,
clipping rejected motion or rebasing time. Final source revalidation uses the
already-held Engine database guard, while native pins retain independent expiry.

Require a separate current-environment acknowledgment against the exact selected
source dossier and revision. It is an operator applicability assertion, not a
machine attestation, evidence acceptance, online control consent or Play. Keep it
in the original request fingerprint. Visible preview retains bounded source-ordered
future output with unchanged deadlines; promotion and duplicate reports cannot
refresh stale estimates.

Require original explicit Bind, Play and Stop intents. Bind consumes readiness and
permits Setup/prefill; Play remains separate. Original replay and restart expose
history without restoring clocks, source pins or delivery. Retain existing journal
v12 custody transfer and exact recovery. This first lifetime is one capture epoch
of at most 30 seconds, with no silent continuation or refresh. Tail handling,
sustained playback, useful motion and actual browser/device qualification remain
open. See [ADR B5-061](../research/phase-b/ADR-B5-061-browser-hsp-live-source.md) and
[the implementation guide](PULSAR_BROWSER_HSP_LIVE.md).


## ARCH204 — exact finite input closure and qualified tail drain

Separate source input closure, native settlement and acknowledged playback tail.
Close names the exact original admitted header and prevents further ingress while
pending CPU work and direct delivery settle. Only joined native custody may mint
an input seal. Retain source/timing pins within their original lifetime; a report
or status poll cannot mint, renew or reconstruct that authority.

Gate finite HSP drain on a separately reviewed optional policy, matching source
and Bind selection, actual Play and final Append acknowledgment, and joined native
command work. Preserve old absent-policy behavior and serialized identities.
Project current phase through the full qualified Stop budget using the same
observations and original bounds; never reset synchronization age or interpolate
an endpoint. Conservative tail passage latches the existing owned Stop immediately,
including during blocked metadata. Dispatch lateness cannot disable stopping or
be described as successful normal completion.

Visible preview may finish its remaining source while exact input is sealed;
its estimate deadlines stay unchanged. This does not close sustained source/media
handoff, ongoing clock acquisition, useful-model or physical-device qualification.
See [ADR B5-062](../research/phase-b/ADR-B5-062-finite-input-and-hsp-tail-drain.md).

## ARCH205 — original preclaim custody across bounded contention

Brief dependency contention may return only the exact unconsumed native command
gate. Release all dependency guards before waiting, keep one fixed first-wait cap
and every original deadline, then reacquire and revalidate actor/source/clock
authority. Never reinterpret a generic refusal as retry custody or repeat network
preflight. Claim initiation permanently ends eligibility for this path; later
refusal preserves the original uncertain outcome and cannot issue another send.

Source admission and final dispatch share one held native timing claim, rather
than independently copying timing state. Native identity and original validity
remain mandatory. Full Engine1,534 and five native CPU/localhost scenarios across
three unchanged-binary runs pass their scoped software checks. Real-device and
ordinary-host timing qualification remain open. See
[ADR B5-063](../research/phase-b/ADR-B5-063-original-preclaim-custody-under-contention.md).

## ARCH206 — stable stream origin with explicit source and clock succession

Continuous analysis retains one stream and original Play while replacing active
source capabilities through a shared native frame, joined predecessor work and an
actual successor pair. Separate session observation ownership preserves preparation
history and original synchronization age; immutable clock pins remain with earlier
effects. Visible media delivery keeps its player and URL, while outstanding reads
retain their old deadlines. Implementation and composed verification are in progress.
See [ADR B5-064](../research/phase-b/ADR-B5-064-continuous-source-and-clock-ownership.md).

## ARCH207 — immutable native timing publication for fresh readers

Publish actual native timing and source custody together so fresh readers do not
wait on observation-copy locks. Preserve original observation bounds and distinct
captured-start retirement; final command claims still serialize and verify the
exact native registry record. The original native failure is retained, and rebuilt
verification is pending. See
[ADR B5-065](../research/phase-b/ADR-B5-065-native-timing-publication.md).


## ARCH208 — explicit native browser HSP control lifecycles

Proposed: bind browser HSP actions to a typed native analysis-source selection and
retain separate bounded mutation, inspection/status and Stop/cancellation ownership.
Keep original request identities across delay and uncertainty. A fresh control
lifecycle requires explicit inspection after the prior work and cleanup have settled;
a bounded archive preserves those original identities without restoring authority.
Device/session credentials and owner proof stay native; the page receives scoped
observations and supplies separate explicit actions. Hosted HSSP acceptance cannot
substitute for generated HSP qualification, and all existing Engine admission gates
remain unchanged. Source transfers preserve one stream and original Play.

Isolated mock-DOM JavaScript and formatting checks do not establish compiled native
or composed rendered control, physical qualification or roadmap completion. See
[ADR B5-066](../research/phase-b/ADR-B5-066-browser-hsp-controls.md) and the
[proposed UI handoff](evidence/roadmap-2026-10-23/hsp-continuity/product-ui-next/implementation/README.md).


## ARCH209 — observation selection before fresh clock sampling

Proposed: fresh playback evaluation selects the original immutable browser
publication and native clock pin before one new evaluation-time sample, then returns
that tick with its facts/snapshot. Consumers and wrappers use the returned tick.
Ordinary facts do not require future Stop coverage; finite-tail snapshots retain
the same selected inputs for their projection. Fixed-at reads, final guarded claims,
captured-start retirement and earlier effect references remain unchanged. Refresh
until, predecessor expiry, synchronization ceiling and continuity duration are never
restarted. No generic retry, prior-publication fallback or grace period is added.

The retained native diagnostic confirms the sampling/selection race (four scenarios
passed, one failed); repaired runtime verification remains pending. Required
interleaving, expiry and cancellation regressions are specified in
[ADR B5-067](../research/phase-b/ADR-B5-067-selection-before-clock-sampling.md), which
refines [B5-065](../research/phase-b/ADR-B5-065-native-timing-publication.md) without
changing its original publication decision. No physical or release acceptance is
claimed.


## ARCH210 — certified passage separate from fresh phase uncertainty

Proposed: retain a monotone current-passage floor within one forward-only HSP
stream, while preserving raw current phase and original command facts. A weaker
overlapping interval is compatible; native-time reversal or a fresh upper bound
below certified passage is not. Ring release uses established passage, while
fresh upper limits, raw future horizon and original authority remain binding.
Validated source transfer preserves the coordinate/floor; a new stream resets it.
Finite Stop projections have separate future history and cannot certify current
tail passage. See [ADR B5-068](../research/phase-b/ADR-B5-068-certified-passage-and-phase-uncertainty.md)
and its retained actual false-regression trace. Implementation and qualification
remain unverified by this decision alone.
