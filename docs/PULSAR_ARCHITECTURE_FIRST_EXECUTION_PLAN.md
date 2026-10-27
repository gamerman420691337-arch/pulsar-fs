> Current execution state, priority and verification scope are maintained in the [execution dashboard](PULSAR_MILESTONES.md). This document retains its design/acceptance obligations and historical checkpoints; dated “current” entries below are not the current work queue.

# Pulsar Architecture-First Execution Plan

Version: 1.1
Date: 2026-09-11
Status: Architecture Phase A1-A6 structurally complete; Phase B in progress; release qualification pending
Decision source: ARCH-041, direct product-owner instruction on 2026-09-11

## 1. Authority and changed order

The [Pulsar Software Specification](PULSAR_SOFTWARE_SPECIFICATION.md) remains the product contract. The [architecture record](PULSAR_ARCHITECTURE_DECISIONS.md) defines the selected four-library architecture and ARCH-001 through ARCH-041. The [major roadmap](PULSAR_MAJOR_ROADMAP.md) retains all M1-M8 scope and acceptance obligations.

ARCH-041 supersedes the previous M1-before-architecture execution order:

1. **Phase A: make the architecture right.** Move production paths into the final ownership/dependency structure, scaffold every required interface family, and demonstrate real composed paths.
2. **Phase B: make it good.** Solve functional and technical challenges using explicit research protocols, implement evidence-supported choices, and close correctness defects.
3. **Phase C: make it solid.** Complete adversarial hardening, formal correspondence, platform/hardware qualification and product release gates.

M1-M8 are now scope and acceptance workstreams, not a mandatory chronological sequence. No historical regression, product requirement, compatibility promise or qualification gate disappears because delivery order changes.

"Final architecture" means the agreed dependency and authority structure is present and enforced for enabled production paths. It does not mean empty crates, frozen algorithm internals, complete model accuracy or qualified physical behavior. A later interface change requires an ADR explaining evidence and compatibility impact rather than an unrecorded exception.

Version 1.0 established the delivery sequence without implementation claims. Phase A has now been executed; its scoped results and limitations are recorded in [PULSAR_PHASE_A_STATUS.md](PULSAR_PHASE_A_STATUS.md). Remaining milestone descriptions are obligations, not completed work. No release, neural-quality or physical qualification is implied.

## 2. Difficulty bands

| Band | Difficulty | Typical work | Estimation rule |
| --- | --- | --- | --- |
| D1 | Mechanical / bounded | Modern names, module moves, composition roots, manifests and compatibility aliases. | Estimate after one source inventory; do not confuse many moved lines with research difficulty. |
| D2 | Deterministic correctness | Checked units/geometry, DSP invariants, frame association, codecs and pure transformations. | Bound by characterized behavior, explicit contracts and independent expected results. |
| D3 | Stateful / cross-platform assurance | Durable transactions, concurrency, authority, isolation, resource admission, recovery and packaging. | Split by failure domain and observable transition; include adversarial and platform evidence. |
| D4 | Research / externally constrained | Neural accuracy, calibrated review flags, six-axis inference, vendor performance, physical stopping and statistical qualification. | Use bounded experiments and hardware/data budgets; do not invent calendar certainty before feasibility evidence. |

The hardest work is not the crate layout. It is obtaining trustworthy motion and calibrated error review, meeting end-to-end performance on qualified hardware, and establishing bounded device behavior. Those challenges remain after structural integration.

## 3. Target structure and module ownership

The selected layout is now a workspace with four production libraries, the GUI/CLI/engine executable and a dedicated binary-only worker companion (ARCH-048). The [migration ledger](PULSAR_PHASE_A_MIGRATION.md) distinguishes enabled paths from unsupported implementation and qualification gaps; this is not yet a portable release package.

```text
pulsar executable / composition root
  client role -> pulsar-clients
  engine role -> pulsar-engine
  legacy worker role -> shared isolated role dispatcher

pulsar-worker executable / data-only composition
  worker roles -> isolated adapters using protocol/core contracts

crates/pulsar-core
  domain identities, units, time, geometry and evidence kinds
  pure perception/tracking/DSP/motion kernels
  project/playback state transitions and invariant predicates

crates/pulsar-protocol
  command/query/event schemas and typed errors
  wire framing, compatibility and checked domain conversion
  worker/artifact/buffer interface contracts

crates/pulsar-engine
  per-user authority and lifecycle
  projects, transactions, revision history and provenance
  jobs, attempts, immutable artifacts and resource admission
  isolated media/inference/plugin/driver adapters and effect brokers
  device sessions, qualification admission and diagnostic orchestration

crates/pulsar-clients
  CLI
  PulsarDesktop GUI
  independent preview state and editing gestures
  approval/assistant presentation
```

Allowed dependency direction: clients and engine depend on protocol/core; protocol depends on core; core depends on neither clients nor engine, GUI, I/O or inference runtimes. Third-party/native extension code must not execute in the engine/GUI address space. Workers propose artifacts and cannot commit project state.

| Required part | Structural home | Architecture milestone | Functional/research milestone | Solid/qualification milestone |
| --- | --- | --- | --- | --- |
| CLI/orchestration and GUI | Clients over identical engine interfaces; executable dispatch only. | A1/A3/A6 | B6 | C1/C3 |
| Media input/parser | Core contracts plus isolated engine-managed import/decode adapters. | A2/A4/A5 | B2/B4 | C1/C2 |
| Media viewer | Core association predicates, engine preview analysis, client viewport/playhead. | A2/A4 | B2/B6 | C1/C3 |
| Media editor | Core edit algebra, engine transactions/history, client gesture previews. | A2/A3/A6 | B6 | C1/C3 |
| General DSP | Core pure kernels with explicit units and provenance. | A2/A4 | B2/B4 | C1/C3 |
| Optical flow / Lucas-Kanade / tracking | Core state and kernels; budgeted worker execution. | A2/A4/A5 | B2 | C1/C3 |
| Neural runtime and model parsers | Protocol tensor contracts and isolated native worker adapters. | A4/A5 | B3 | C1/C2 |
| Model zoo and execution profiles | Engine-managed artifact identities, manifests and qualification records. | A2/A5 | B3/B4 | C2/C3 |
| Local assistant | Client proposals; engine capability/approval broker and resource limits. | A3/A5/A6 | B3/B6 | C1/C3 |
| Device profiles, drivers and playback | Core adaptation/admission predicates and engine-owned sessions/brokers. | A2/A5/A6 | B5 | C1/C2/C3 |
| Hardware validation doctor | Explicit configuration/evidence interfaces; read-only probes by default. | A5/A6 | B3/B5 | C2/C3 |
| Internal assurance suite | Kernel/interface harnesses, lifecycle models and correspondence fixtures. | A1-A6 | B1-B6 | C1-C3 |
| Portable artifacts and lineage | Engine artifact interfaces; build/distribution manifests and provenance audit. | A1/A5 | B3/B6 | C2/C3 |

## 4. Phase A: architecture first

Phase A performs real integration, not algorithm optimization. Existing behavioral defects are tracked explicitly; extraction does not certify them acceptable. Data loss, authority bypass, unsafe physical effects and migration-created regressions cannot be deferred behind an "architecture only" label. Disable an unqualified/unsafe path if its minimum guarantees cannot yet be supplied.

### A1. Workspace, ownership and migration inventory

Difficulty: D1 with D3 integration exposure.
Dependencies: none beyond establishing the selected current baseline without overwriting unrelated work.

Deliverables:

- Map existing production entry points, modules, effects and duplicated paths to the four target libraries and process roles.
- Establish the workspace/composition layout and `PulsarDesktop` naming, preserving attribution and documented CLI aliases.
- Record source/model lineage questions without claiming clean-room origin or changing licenses.
- Add architecture dependency rules and a migration inventory identifying each transitional adapter, owner, allowed behavior and removal milestone.
- Retain existing reproduced defects as baseline observations, not extraction regressions or silently accepted behavior.

Exit evidence: each enabled production path has an owner; dependency enforcement exists; compatibility entry points still resolve; no renamed symbol or new crate is counted as functional improvement by itself.

Existing workstreams: M1 baseline/lineage, M2 shared structure, M6 compatibility.

### A2. Checked domain and interface vocabulary

Difficulty: D2.
Dependencies: A1 ownership map.

Deliverables:

- Distinct source version/placement/frame, entity/target/output, revision/job/attempt/artifact, grant/session and physical-device identities.
- Rational source timestamps, checked project nanoseconds, separate monotonic deadlines and explicit coordinate-space transforms.
- Checked neutral-axis values, unavailable/predicted/inferred/synthesized evidence, compact provenance references and protected-region semantics.
- Pure state transitions for edits, candidates, attempts and playback admission; no I/O or runtime initialization inside core.
- Explicit interface inventories for media, inference, tracking, motion, persistence, jobs, exports, authority and devices.

Exit evidence: enabled callers cross checked interfaces; invalid values fail explicitly; missing observations/axes cannot silently become observed motion/zero values; core forbids unsafe code and forbidden dependencies.

Existing workstreams: M1 geometry/time, M2 domain, M4 axes.

### A3. One authoritative engine and one real vertical path

Difficulty: D3.
Dependencies: A1/A2.

Deliverables:

- Per-user engine discovery/instance exclusion and secured versioned local transport.
- Paired/scoped clients, typed commands/queries/events/errors, expected revisions and request identities.
- Bundled SQLite project transactions, durable acknowledgement, actor-labelled undo/redo and protected-edit enforcement.
- Minimal real path shared by CLI and GUI: import an immutable input, start work, receive a revision-bound candidate, commit through the engine and export neutral motion.
- Cancellation/revocation/reconnect semantics with explicit ordering; no worker or client direct project writes.

Exit evidence: both clients exercise the same engine operation; accepted edits have durable recorded outcomes; stale/unauthorized commits reject; the vertical path is not a fake-success stub. Advanced model quality is not an A3 gate.

Existing workstreams: M2 projects/API/client parity.

### A4. Media, perception, DSP and viewer integration

Difficulty: D2/D3.
Dependencies: A2 and the A3 vertical path.

Deliverables:

- Route existing import/decode, detector/flow/tracker, motion-processing and export implementations through their assigned interfaces.
- Replace duplicated orchestration with one shared execution path; preserve behavior provenance while identifying known algorithm defects.
- Carry source/frame/transform/request identities to client observation rendering, with explicit unavailable and prediction states.
- Separate preview playheads, transient edit gestures and engine-owned physical playback.
- Scaffold all required modality and six-axis interface families; unsupported implementations report unsupported and remain visible delivery gaps.

Exit evidence: actual observations can reach the viewer through the intended path; seeks/cuts/transforms have explicit invalidation contracts; no production caller bypasses the shared path merely to retain an old implementation. Detector quality and full bug closure remain B work.

Existing workstreams: M1 consumer paths, M2 orchestration, M4 modality interfaces.

### A5. Workers, artifacts, extension roles and resources

Difficulty: D3.
Dependencies: A2/A3; integrate with A4.

Deliverables:

- Isolated worker roles with immutable manifests, distinct attempts, bounded messages/buffers and atomic artifact/checkpoint publication.
- Snapshot/pinning/eviction interfaces preserving active dependencies, original media, committed motion and compact lineage.
- Runtime/model/plugin/driver role manifests and capability brokers, with no engine/GUI in-process extension loading.
- Aggregate CPU/RAM/VRAM/storage admission, bounded retries and separate host-call deadlines.
- Explicit interfaces for qualified fallback, strict pins, custom unqualified runs and interrupted-job recovery.
- Assistant action proposals use the same scoped engine interfaces, not shell access or direct GUI/project-memory mutation.

Exit evidence: a real worker path obeys budgets and authority; malformed/late responses fail closed; unsupported confinement or backend paths stay disabled unless the selected exact-build human exception applies. Empty scheduler or sandbox interfaces are insufficient.

Existing workstreams: M2 jobs/artifacts, M3 runtime/resources, M5 driver workers, M6 packaging prerequisites.

### A6. Whole-architecture closure

Difficulty: D3.
Dependencies: A1-A5.

Deliverables:

- Complete the interface inventory and ownership graph for every required logical module.
- Route all enabled GUI/CLI/batch/project/device effects through the designated engine authority and workers.
- Install device admission/controller/session and assistant approval interfaces without enabling unqualified physical capabilities.
- Remove duplicate production authority paths. Keep only explicitly scoped compatibility/implementation adapters with removal milestones.
- Establish lifecycle models, correspondence-test entry points and invariant harnesses alongside their implementations.
- Publish a capability ledger separating implemented, unsupported, unqualified and disabled capabilities.

Exit evidence: the architecture gate below is met. A6 is structural integration, not Preview readiness.

Existing workstreams: M2 integration plus structural prerequisites across M1-M6.

### Phase A exit gate

All required criteria must have subject-bound evidence when Phase A is executed:

1. Four-library dependency direction and core restrictions are enforced, not merely documented.
2. Every enabled production path has a module owner and uses its designated interface; no client/worker direct project commits or ambient device effects remain.
3. GUI and CLI complete the same real import -> worker -> candidate -> durable commit -> neutral export path.
4. Checked identity, time, coordinate, revision and evidence types cross that path without silent semantic conversion.
5. Authentication, expected revisions, revocation/cancellation fencing and durable outcomes are integrated for enabled operations.
6. Workers and bulk artifacts are isolated, bounded and ownership-scoped; unsupported enforcement is explicit.
7. Viewer observations are associated with displayed context; unavailable evidence is not represented by a decorative tracking box.
8. Device and assistant authority interfaces are present; unqualified or unsupported effects cannot be activated accidentally.
9. Remaining adapters have named owners, bounded responsibilities and removal milestones; duplicated authoritative behavior is gone.
10. Known defects and unimplemented capabilities remain in the ledger with later milestones. Structural integration does not mark them resolved.

## 5. Phase B: make it good through evidence-led research

All B work starts after A6. Independent B tracks may run concurrently once their prerequisites and resource/data budgets are established. Deterministic bugs that invalidate a research baseline must be corrected before trusting measurements; there is no reason to conduct deep research on an already demonstrated divide-by-zero.

### B1. Research baseline, defect map and evaluation protocol

Difficulty: D2/D4.
Dependencies: A6.

Deliverables:

- Reproduce the historical 20 regression cases against the migrated, selected baseline; record fixed, still failing, changed and no-longer-applicable cases separately.
- Map major flaws and security hypotheses to interfaces, violated invariants, triggers, reachability, consequences and evidence.
- Run the authorized five-human-hour annotation/review pilot when explicitly executing that work; preserve independent expected results and record achieved coverage.
- Define held-out splits, category taxonomy, uncertainty reporting and proposed statistical release rules; request human decisions for unapproved budgets or product thresholds.
- Freeze a useful baseline for each research question and create its experiment protocol before tuning.

Exit evidence: reproducible defect/benchmark inventory, independent oracles and explicit unresolved acceptance definitions. A five-hour pilot is not release qualification.

Existing workstreams: M1 regression evidence, M3 benchmark foundation, M7/M8 evaluation design.

### B2. Stroke, perception, tracking and truthful review flags

Difficulty: D4 research plus D2 correctness.
Dependencies: B1.

Deliverables:

- Correct detector shape/coordinate validation, clipping, track identity/aging/cuts, box propagation and forward/backward-flow handling.
- Preserve constant input, shallow motion, direction, reversal timing, pauses and plateaus; remove invented full-range movement.
- Distinguish camera/target motion, occlusion, missing evidence and genuine synthesis.
- Investigate tracking/fusion/uncertainty alternatives using controlled comparisons and independent references.
- Connect confidence/review behavior to materially incorrect sections and actual human review burden, not only score thresholds.
- Exercise moving targets, no detections, seeks, cuts, resizing, projections and late results in the real GUI path.

Exit evidence: M1 correctness cases resolved with independent regression coverage; real-media results separately reported from synthetic fixtures; quality and review-flag claims supported on their declared evaluation subjects. Whole-release accuracy remains a C3 gate.

Existing workstreams: M1, M3 quality, M4 projection/evidence semantics.

### B3. Local inference, model zoo and performance

Difficulty: D4.
Dependencies: B1 and stable B2 measurement interfaces; B2/B3 may iterate together.

Deliverables:

- Compare model weights, preprocessing/postprocessing, quantization, runtimes and per-vendor execution profiles.
- Measure end-to-end CPU/GPU throughput, peak RAM/VRAM, preparation/extraction effects and cold/warm behavior.
- Support the bundled offline assistant and generation without compromising live/control reserves.
- Qualify equivalent fallback within the requested tier; retain strict backend pins and prominent custom-run status.
- Audit model/runtime redistribution and artifact provenance before packaging a selected candidate.

Exit evidence: selected profiles have reproducible quality/speed/memory evidence on identified configurations. Unsupported hardware remains explicitly unqualified, not silently CPU-backed while claiming acceleration.

Existing workstreams: M3 inference/performance, M6 vendor feasibility.

Ordinary Linux worker pipe supervision now shares one deadline across request writes, response/artifact/EOF reads and exit polling, with bounded retained diagnostics and no reader-thread joins. See [worker pipe deadlines](PULSAR_WORKER_PIPE_DEADLINES.md). [Ordinary worker retirement custody](PULSAR_WORKER_RETIREMENT.md) now retains the admitted reservation until direct-child exit and an empty cgroup are observed, with unresolved ownership transferred to a reaper. Blocking host IO, durable restart fencing and production GPU/native-image/device lifecycle remain open; this does not enable engine GPU jobs.

### B4. All modalities, synthesis and six axes

Difficulty: D4.
Dependencies: B1 plus usable B2/B3 profiles.

Deliverables:

- Complete required video/projection, audio, text, still-image-plus-prompt, mixed and live-input interface implementations according to the specification.
- Keep faithful reconstruction and creative synthesis separate, including evaluation methods.
- Complete the neutral six-axis program while exposing unobservable/inferred motion honestly.
- Preserve the stroke-only scope of offline speed floors; preserve the required GPU fastest-preset live six-axis capability.
- Establish proposed and approved quality conventions for creative, Fast/Core, Maximum and secondary-axis outputs without inventing prior user approvals.

Exit evidence: each required input/output family has real functional evidence, provenance and a declared evaluation method. Missing modalities block their required release scope.

Existing workstreams: M4, M3 model coverage, M5 live-input prerequisites.

### B5. Live scheduling, profile adaptation and device integration

Difficulty: D4 with high-consequence D3 state.
Dependencies: B2/B3 and the relevant B4 live-output path.

Deliverables:

- Integrate neutral -> adaptation -> admission -> encoding -> transport, preserving timing and bounding amplitude.
- Implement required Handy/Handy 2 online/offline modes and the selected device qualification flow.
- Exercise controller transfer/loss, revision switching, command age, reconnect, stop/disarm and unknown physical state.
- Measure capture-to-command latency separately from physical response; investigate sustained backlog and non-preemptible workloads.
- Establish configuration-bound qualification and stopping evidence through explicitly authorized physical work; human acceptance remains separate.

Exit evidence: device capabilities have exact driver/device/firmware/transport evidence, no implicit rearm, and separately approved headless eligibility. A mocked stop does not qualify a physical device.

Existing workstreams: M5; M4 live six-axis integration.

### B6. Product integration and functional defect closure

Difficulty: D3.
Dependencies: B2-B5 as applicable; independent editor/assistant usability work may proceed after B1.

Deliverables:

- Complete editor gestures, protected regions, actor-labelled history, candidate review/rebase, project packaging and diagnostics.
- Complete the bounded local assistant and plugin workflows, including per-invocation native approval and the separately risky standing mode.
- Remove phase-B transitional implementations when their replacement gates pass.
- Close remaining functional defects across GUI/CLI parity, state, worker/resource behavior and user-facing error recovery.
- Re-run affected comparisons when integration changes model inputs, transformations, scheduling or output behavior.

Exit evidence: required functional scenarios succeed through production interfaces; remaining defects are classified and cannot be hidden by a successful demo. Phase B functional demonstration does not certify adversarial robustness or release readiness.

Existing workstreams: M1-M5 functional completion, M6 export/update preparation.

## 6. Phase C: make it solid

Assurance harnesses begin in Phase A; Phase C completes their adversarial depth and release qualification. Deferring all tests, security design or proof correspondence until Phase C is explicitly not this plan.

### C1. Correctness, security and adversarial hardening

Difficulty: D3.
Dependencies: relevant B implementations.

Deliverables:

- Complete KAT, property/metamorphic/differential, parser/protocol/manifest fuzzing and mutation campaigns.
- Exercise crash/disk-full/OOM/timeout/revocation/reconnect/duplicate-request and artifact-tampering failures.
- Complete bounded Kani obligations and TLA+/TLC lifecycle models with implementation-correspondence evidence.
- Audit native/assistant/plugin effects, buffer isolation, dependency identity and stale authority across actual platform implementations.
- Resolve release-blocking defects; bind residual limitations to explicit assumptions and capability exclusions without silently reducing promised release scope.

Exit evidence: critical declared invariants have required proof/test evidence; adversarial findings are resolved or remain explicit blockers. Model checking does not establish neural accuracy, foreign-runtime correctness or physical stopping.

Existing workstreams: M2-M6 hardening; mandatory M7 formal prerequisites.

### C2. Portable release and platform/hardware qualification

Difficulty: D3/D4.
Dependencies: B3/B5/B6 and applicable C1 gates.

Deliverables:

- Qualify offline Core/complete packages, verified first-use extraction, interrupted update/rollback and consistent project migration/export.
- Exercise clean supported machines without user-installed developer/inference tools or mandatory first-run model downloads.
- Qualify required OS/architecture/GPU-vendor configurations and document actual OS/driver prerequisites.
- Complete signing/distribution and source/model/runtime attribution/redistribution evidence.
- Bind device records and approvals to exact configurations; requalify changed artifacts and firmware where required.

Exit evidence: candidate-bound platform matrix and physical records, not compilation/provider-detection claims. Unsupported release promises remain blockers for that required release scope.

Existing workstreams: M5 physical qualification, M6 portability.

### C3. Preview, Stable and comparative qualification

Difficulty: D4.
Dependencies: C1/C2 and the applicable product gates.

Deliverables:

- Close the M7 Preview gate using the specification's quality/performance scope plus critical formal obligations.
- Close the M8 Stable gate using independent held-out evaluation, per-category human-effort evidence and required physical qualification.
- Keep flags' material-error recall distinct from generation precision/recall and review burden.
- Report Fast/Core, Default, Maximum, creative and secondary-axis evidence separately according to their approved criteria.
- Make SOTA claims only from dated, reproducible comparisons with disclosed inputs, hardware and metrics.

Exit evidence: required candidate-bound release report, no unresolved release-blocking defects and no missing required qualification. Preview success does not automatically establish Stable or comparative superiority.

Existing workstreams: M7/M8.

## 7. Research and ADR discipline

Research follows Phase A and uses stable measurement interfaces. Research may refine internals freely; evidence-driven changes to shared contracts require an explicit ADR and compatibility review.

Every D4 investigation follows this loop:

1. Define the research question, falsifiable hypothesis and contract it must satisfy.
2. Review primary papers, official documentation and relevant source implementations; record versions, limitations and provenance.
3. Freeze a baseline, independent oracle, dataset split and candidate artifact identities.
4. Predeclare metrics, tolerances, resource/time/data budgets and stopping conditions before comparing candidates.
5. Run bounded local experiments; preserve raw/derived results, scripts, seeds, exact dependencies and failure cases.
6. Compare quality, throughput, memory, human effort and portability jointly; reject improvements that silently violate another confirmed requirement.
7. Write the decision ADR with evidence links, rejected alternatives, tradeoffs and any unresolved assumptions.
8. Implement the chosen result, add regression coverage, and repeat affected evaluation through production interfaces.

Negative results and "no candidate met the contract" are valid scientific outcomes, not permission to lower the contract. External compute spending, training budgets, private-data disclosure and physical actuation require their own authorization.

ADRs record decisions and reference the science. Experiment records retain protocols and results; an accepted ADR is not a passed experiment or a qualification receipt. Maintain both so negative findings and reproduction details survive later summaries.

Recommended durable records, to be created with the corresponding work:

| Record | Required content |
| --- | --- |
| Decision ADR | Status, decision source, question, constraints, alternatives, chosen option, evidence references, consequences, interface impact, validation obligations and supersession. |
| Experiment protocol | Hypothesis, baseline/candidates, exact artifacts, data split, independent oracle, metrics/tolerances, budget and stop conditions. |
| Experiment result | Environment, source/build identities, commands, seeds, measurements, uncertainty, negative results, artifacts and reproduction instructions. |
| Qualification receipt | Exact subject/configuration, required gate, tool/protocol version, result, limitations, invalidation rules and required human acceptance. |
| Defect record | Requirement/interface, invariant, trigger, reachability, impact, evidence status, fix milestone, regression case and closure evidence. |

Document directories and record naming must use the repository's existing conventions when implementation begins; this planning step does not invent completed ADRs, experiments or acceptance.

## 8. Bugs are tracked throughout, not postponed blindly

Use explicit defect evidence states:

- Historically reproduced: demonstrated against an older recorded baseline.
- Currently reproduced: demonstrated against the identified current implementation.
- Source-supported: code reasoning establishes the behavior under stated preconditions.
- Hypothesis: investigation required; not a confirmed vulnerability.
- Specification/acceptance gap: behavior or required evidence lacks a settled measurable definition.
- Qualified closed: fix and required regression/integration evidence match the actual subject.

Phase A fixes migration blockers, data-loss/authority hazards and unsafe enabled effects; otherwise disable the path and retain the blocker. Phase B fixes deterministic defects that contaminate research before measurements are trusted and closes functional defects alongside research. Phase C closes adversarial, release and qualification defects.

The previously observed 20 cases remain an explicit M1 inventory: eight detector-decoding cases, four tracker cases, three point-tracker cases, four stroke-processing cases and one flow-dimension case. The stationary GUI region/observation issue is additional historical source evidence. These are not fresh failure counts for the migrated/current checkout.

## 9. Contract-to-execution crosswalk

| Architecture contract | Architecture placement | Good/function/research | Solid/qualification |
| --- | --- | --- | --- |
| CONTRACT-01 Modules and modern names | A1/A6 | B6 adapter retirement | C1 dependency/compatibility audit |
| CONTRACT-02 Identities, units and evidence | A2/A4 | B2/B4 algorithms | C1 arithmetic/association evidence |
| CONTRACT-03 Viewer and tracking | A4 | B2/B6 | C1/C3 real-media/UI evidence |
| CONTRACT-04 Projects and persistence | A3/A5 | B6 | C1/C2 crash/migration/export evidence |
| CONTRACT-05 Engine, jobs and resources | A3/A5 | B3/B5/B6 | C1/C2 stress/recovery qualification |
| CONTRACT-06 Local interface and authority | A3/A5/A6 | B6 | C1 protocol/revocation/replay campaigns |
| CONTRACT-07 Provenance, privacy and disposal | A2/A5 | B1/B3/B6 | C1/C2 retention/export/lineage evidence |
| CONTRACT-08 Plugins, models and runtimes | A5/A6 | B3/B4/B6 | C1/C2 confinement/vendor/distribution evidence |
| CONTRACT-09 Device admission and qualification | A2/A5/A6 | B5 | C1/C2/C3 physical/release evidence |
| CONTRACT-10 Assurance | A1-A6 harnesses/models | B1-B6 independent functional evidence | C1-C3 required deep/formal/release gates |
| CONTRACT-11 Migration and compatibility | A1/A4/A6 | B6 | C2/C3 public compatibility/release gates |

## 10. Status vocabulary and next executable milestone

[Native annotated binding](PULSAR_ANNOTATED_NATIVE_BINDING.md) now verifies typed recipe/intake metadata and named tensors and matches Python on all 14 authored CPU pairs. Shared declaration migration preserves four native planner cases. Workspace, CUDA-feature worker, current-planner Python and architecture gates pass. This explicit adapter does not enable annotated public generation: source-bound profile/settings/view application and retained correspondence are next. Real annotations/alignment, learned quality and independent qualification remain open.

[Source-bound annotated file workers](PULSAR_ANNOTATED_FILE_WORKER.md) now implement the explicit preprocessing profile, source-hash view settings, shared native view application and schema-3 plan/model lineage. Authored stereo decoding reproduces all 28 Python input frames exactly and all 14 intended estimates within 1e-6. Engine publication, Stash batch and model-pack admission remain guarded pending durable reader barriers and portable/recovery correspondence; first-party client selection and actual public-path acceptance remain open. See [ADR B2-006](../research/phase-b/ADR-B2-006-source-bound-annotated-file-worker.md).

[Separate annotated model export](PULSAR_ANNOTATED_MODEL_EXPORT.md) now binds native-view staging to a distinct named ONNX input and versioned recipe/intake metadata, with explicit development groups and planner replay. Authored export/lineage/refusal gates and all 31 reference Python tests pass. Production annotated profile/runtime/generation binding is next; real annotations/alignment, useful quality and independent qualification remain open.

[Explicit annotated training views](PULSAR_ANNOTATED_TRAINING_VIEWS.md) now stage source-bound eye/projection and static target-region inputs through the shared native view planner. Authored flat/full/half equirectangular native cases and source/geometry/output/export fences pass, along with serial workspace and CUDA-feature worker gates. Existing models remain whole-frame; separately binding the annotated model/export/profile/runtime contract is the next software requirement. Real annotations, visual/script alignment and useful quality remain open.

[Existing training pixel/PTS correspondence](PULSAR_TRAINING_PIXEL_CORRESPONDENCE.md) now passes for all 32 saved clips and 2,860 consumed temporal pairs using single-pass native RGB/framehash output and exact stored tensor comparison. All previous sources/data/models remain unchanged. Target/projection-aware inputs, independent visual/script alignment and useful model quality remain open; correspondence is not quality qualification.

[Temporal training orientation](PULSAR_TEMPORAL_TRAINING_ORIENTATION.md) now matches runtime encoded-source decoding for future intake. Authored Python and native Rust regressions pass, while all eight hash-verified existing training videos lack rotation metadata. This repair does not explain the saved model failure. Existing data/models remain unchanged; the subsequent pixel audit verifies correspondence for consumed samples. Target/projection-aware inputs and visual/script alignment are the next quality investigations.

[Live-reference label consistency](PULSAR_LIVE_LABEL_CONSISTENCY.md) independently rebuilds all 2,860 stored labels exactly from declared script clocks. Between-frame script excursions occur in 1,287 sampled pairs; this is a sampling diagnostic, not a proven cause of model underfitting. Target/projection declarations and verified visual/pixel-clock alignment remain absent or unverified. No model, labels, offsets or sterile inputs changed; quality and independent acceptance remain open.

[Desktop retained CUDA removal](PULSAR_DESKTOP_CUDA_EVIDENCE_REMOVAL.md) provides inspection-bound confirmation, background dispatch and exact pending retry across navigation. [Durable removal recovery](PULSAR_DESKTOP_CUDA_REMOVAL_RECOVERY.md) now saves intent before dispatch, restores without automatic replay and retains validated acknowledgements. The serial workspace/default build, owned-child SIGKILL preparation recovery and selected native worker reopen/refusal/retry campaign pass. Actual window/human QA, physical power-loss/broader crash cuts, disposal policy and secure erasure remain open.

[Retained CUDA removal](PULSAR_RETAINED_LIVE_CUDA_REMOVAL.md) adds explicit CLI/API logical disposal bound to current Read and project-administrator authority, current revision and manifest intent, with durable receipt replay. The source-bound serial workspace/CUDA gates and selected native CPU CLI refusal/download case pass. Desktop removal, disposal policy and secure erasure remain open.

[Live-development signal measurement](PULSAR_LIVE_DEVELOPMENT_SIGNAL.md) now compares a predeclared fixed retrieval baseline with the frozen six-group candidate and constant baselines, without reading sterile inputs or fitting a model. Cross-group retrieval and learned estimates remain worse than a constant predictor; same-source correlation does not establish generalization. Exact-pair/source-byte leakage was not found, while family/label alignment remains unverified. Target/pose and independent quality evidence remain open.

[Generation quality diagnosis](PULSAR_GENERATION_QUALITY_DIAGNOSIS.md) reproduces the frozen temporal candidate’s zero-match result and attributes an unavoidable travel failure to its narrow recorded model output range. All 170 direct estimates exactly match saved production positions, and all 171 input PTS match at stride one. This is negative model evidence, not a code fix or quality pass. Further model work must use permitted training/development data without sterile tuning; alignment and independent acceptance remain open.

[Evidence worker failure handling](PULSAR_DESKTOP_EVIDENCE_WORKER_FAILURE.md) now clears a busy/stale panel and disables requests when its background worker disconnects, without inferring whether an in-flight export published. Software regression and native positive-worker checks pass. Actual isolated-window QA remains unverified because the required Agent Workspace tools are not exposed in this session; retention controls and broader roadmap obligations remain open.

[Desktop capture evidence](PULSAR_DESKTOP_CAPTURE_EVIDENCE.md) now provides background browsing, original-record inspection and five-part artifact export with scoped view invalidation and shared no-overwrite transport. The actual desktop worker passes against retained CUDA evidence, and headless egui widget construction passes. Actual window interaction/human usability and retention controls remain open alongside the broader roadmap.

[Retained CUDA capture browsing](PULSAR_RETAINED_LIVE_CUDA_LISTING.md) now exposes a project-authorized, revision-checked scalar index through the CLI, with bounded epoch cursor pagination. The CPU-only shipped CLI discovers retained CUDA evidence after restart and downloads all five originals. Index entries are not original-evidence validation. Retention controls, actual desktop-window qualification and remaining product/qualification gates stay open.

[Original retained CUDA downloads](PULSAR_RETAINED_LIVE_CUDA_DOWNLOAD.md) now expose all five original artifacts through existing authenticated bounded leases and a no-overwrite CLI publication guard. Validated originals remain memory-admitted through retries and retire on abandonment/revocation/expiry. Actual CPU-only CLI outputs match preceding original SQL/file bytes. Retention controls, actual desktop-window qualification and remaining product/qualification work stay open.


[Retained live CUDA inspection](PULSAR_RETAINED_LIVE_CUDA_INSPECTION.md) now exposes historical descriptors through the project-authorized CLI after registry retirement/restart, including a CPU-only reader. Current Read authority and revision remain mandatory; original paths are not opened. Original-byte downloads now have the successor checkpoint above; retention controls and actual desktop-window qualification remain open alongside the remaining roadmap gates.


The latest [shipped CUDA CLI cancellation checkpoint](PULSAR_CUDA_CLI_FINISH_CANCELLATION.md) covers Finish held pending by a frozen exact owned worker, public Stop, five identical terminal/replay reports, no cancelled evidence and a successful fresh capture on the same Engine. Only verification JavaScript changes. GUI cancellation, additional authority/deadline races, remaining recovery cuts and product/qualification deliverables remain open.


Use **structurally integrated -> functionally demonstrated -> qualified**. These states are not interchangeable and are recorded per capability/configuration.

Current execution is recorded in [PULSAR_ROADMAP_EXECUTION_2026-10-01.md](PULSAR_ROADMAP_EXECUTION_2026-10-01.md); its latest source-bound checkpoints supersede historical counts and scoped limitations. Phase B remains in progress. Explicitly configured Linux CUDA generation/resume and detector Preview have native evidence; persistent live CUDA has private Engine-owned startup, runtime-issued work, provisional reporting, original-byte evidence admission and a conditional schema21 store. The [private consuming finalizer](PULSAR_LIVE_CUDA_FINALIZATION.md) now checks current capture authority, freezes intake under continuing lease supervision, consumes the actual owned worker and links a completed report to committed native evidence. [Public live CUDA capture](PULSAR_PUBLIC_LIVE_CUDA.md) now implements conditional authenticated Start, automatic scheduling and asynchronous Finish, with separate capture provenance in schema22 and a native in-process Engine API campaign. [Selected native explicit and derived epoch handoffs](PULSAR_LIVE_CUDA_CONTINUATION.md) now pass after actual inference. [Exact inactive admission replay and selected native graceful Engine restart](PULSAR_CAPTURE_RESTART_RECEIPTS.md) now pass. [CLI exact-old-boot replay and a selected CPU Engine SIGKILL recovery campaign](PULSAR_LIVE_CAPTURE_CLI_REPLAY.md) now pass over actual local IPC. [A selected native CUDA CLI/local-IPC owner SIGKILL and fresh Finish campaign](PULSAR_CUDA_CLI_RECOVERY.md) now passes. [Selected native consuming-finalizer Stop and real lease expiry around commit](PULSAR_LIVE_CUDA_FINALIZER_RACES.md) now pass with test-build pauses. [A selected injected WAL-sync stall inside actual SQLite commit](PULSAR_LIVE_CUDA_SQLITE_SYNC.md) also passes, refusing Completed after real lease expiry while retaining original historical bytes. [Stop during an actual blocked native worker Finish](PULSAR_LIVE_CUDA_BLOCKED_FINISH.md) now passes, with owned-group freezing, observed reply WouldBlock, bounded cancellation and no retained completion evidence. [The selected public Start/automatic-scheduler/Finish/Stop variant](PULSAR_PUBLIC_CUDA_BLOCKED_FINISH.md) now passes without a test-inserted registry entry, retaining exact terminal replay and no completion evidence. Additional scheduler authority/deadline and CLI/UI variants remain open. Further preparation/inference/finalizer/commit death, UI campaigns, startup/in-flight/cleanup-overlap handoffs, power loss and exhaustive recovery remain open. Known parallel-test failures retain unproven causes despite passing serial gates. B1 quality/alignment, multimodal/six-axis and performance, product integration, real-media, Handy, platform, human and Preview/Stable release gates remain open. A1-A6 structural integration passed the scoped local gates in [PULSAR_PHASE_A_STATUS.md](PULSAR_PHASE_A_STATUS.md); neither those gates nor private authored CUDA execution establish Phase B completion or release qualification.

The subsequent [checked staging cleanup subject](PULSAR_STAGING_CLEANUP_CUSTODY.md) changes five paths and preserves 311 archived source records. Focused cleanup, rebuilt controlled live-read cases and all 108 application cases pass. Its initial broader native gate failed with persistence ENOSPC errors and one historical startup timeout; the unchanged retry passed 497 native cases and 943 workspace cases (24 ignored). The CUDA-feature disk run failed 75 cases with ENOSPC; a same-binary RAM diagnostic passed 481 cases (18 ignored). On October 7 all 311 source records still match, 232 structural checks pass, and the fresh CUDA-feature gate passed 481 cases (18 ignored) after restoration of missing cached dependencies. Cached validation binaries and the historical reader are unavailable; retained earlier identities do not establish current availability. Preceding gate counts above remain source-bound historical evidence. Creation journaling and staging recovery after process death remain the next ownership requirements.

The subsequent [schema-19 staging reader requirement](PULSAR_STAGING_READER_REQUIREMENT.md) preserves an actual preceding application and adds mandatory binding/minimum-ledger checks before recovery and live admission. A missing-column regression rejects a layout-only shortcut. Arming remains fixture-only until creation-error custody and format-3 staging ownership are integrated. The existing actual engine-primitive staging crash regression remains unresolved; these reader checks do not close that gate.

## October 8: annotated generation reader barrier

[Conditional schema-23 storage](PULSAR_ANNOTATED_GENERATION_STORAGE.md) now binds immutable annotated attempts to canonical request digests and validates bounded source/profile/dependency/job/retry metadata before writable recovery or custody. Existing schema-20/21/22 layers coexist, and later CUDA publication preserves header 23. Authored reopen, corruption refusal with unchanged database/WAL/custody bytes, rollback and coexistence pass. The final workspace gate passes 1,079 tests (60 ignored). Public annotated generation remains guarded pending strict schema-3 receipt correspondence, portable closure and first-party controls. No user database migration, real fitting, sterile access or physical playback occurred. Quality, Stash/product closure, platforms, Handy, human and release gates remain open. Roadmap status: PARTIAL.

## October 8: annotated receipt and model correspondence

[Annotated receipt validation](PULSAR_ANNOTATED_GENERATION_RECEIPTS.md) now checks immutable request/result metadata, native planner replay, exact configuration/dependency/review correspondence and content-bound ONNX recipe/intake metadata before candidate publication. Its shared reader entry point keeps historical paths inert. Mutation/bounds tests and authored native receipt/model checks pass; the final workspace gate passes 1,084 tests (60 ignored). Public annotated generation and portable export/import remain guarded. Schema-3 source-clock support, a portable reader barrier and retained model-object closure are next. No fitting, real/sterile access, production migration or physical playback occurred. The [checkpoint](evidence/roadmap-2026-10-08/annotated-generation-receipts/checkpoint.json) retains source-bound gates. Quality, Stash/product closure, platforms, Handy, human and release acceptance remain open. Roadmap status: PARTIAL.

## October 8: portable annotated generation and source clocks

[Conditional format-11 annotated closure](PULSAR_ANNOTATED_PORTABLE_GENERATION.md) retains exact model bytes and schema-3 source clocks through actual Engine import, restart, re-export and a second clone. Authored native verification preserves 28 transformed frames and 14 Python-corresponding estimates; missing membership and four rehashed clock corruptions refuse. A transactional schema-24 wrapper preserves prior CUDA/worker/local-annotation requirements. Older-reader and changed-retained-model refusal preserve observed database/WAL bytes; the latter does not establish unchanged earlier custody/filesystem preparation. Full gate results and source identities are recorded in the [checkpoint](evidence/roadmap-2026-10-08/annotated-portable-generation/checkpoint.json). Public Engine generation and first-party controls remain guarded and are next. No live/sterile fitting or physical playback occurred. Quality, Stash/product closure, platforms, Handy, human and release acceptance remain open. Roadmap status: PARTIAL.

## October 8: public annotated file generation

[Explicit CPU file generation](PULSAR_ANNOTATED_PUBLIC_GENERATION.md) now checks the imported source binding before model snapshotting and retains annotated attempts through the existing local reader barrier. CLI profile/view controls and desktop load/clear controls are implemented. The authored public Engine campaign passes actual confined generation, request replay, commit, persisted-session restart, production capture and portable clone/re-export. A shipped CLI/local-IPC campaign passes profile creation, generation and export with all 14 unrounded Python positions exact, schema-3 receipts and unqualified status preserved. Final gates and source identities are retained in the [checkpoint](evidence/roadmap-2026-10-08/annotated-public-generation/checkpoint.json). Generic Stash/model-pack and annotated CUDA admission remain guarded. Actual desktop usability, real annotations/alignment and useful quality, broader fault/retry campaigns, Stash closure, platforms, Handy, human and release gates remain open. No fitting, live/sterile input or physical playback occurred. Roadmap status: PARTIAL.

## October 8: annotated Stash publication and portable download

[Annotated Stash publication](PULSAR_ANNOTATED_STASH_PUBLICATION.md) now passes through the shipped CLI and actual native CPU worker against an owned loopback Stash fixture. Both stale package-store ceilings now accept format 11 while unknown formats refuse. Actual CLI export/download/import, fresh exact-byte clone association, archived publication with original attempt/model paths unavailable, and restart replay pass. All 28 sample clocks and 14 exact Python estimates are preserved; explicit source-time selection and an explicitly resolved trailing gap remain required. Unknown scan acknowledgement stays separate from attachment observation and is never redispatched. Final gates pass 1,093 workspace tests (62 ignored), four native Stash campaigns, the default build, 283 architecture checks and 36 source guards. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-stash-publication/checkpoint.json). Installed Stash compatibility, generic annotated batches/model packs/CUDA, mixed clocks without piecewise proof, useful learned quality, desktop, platforms, physical Handy and human/release gates remain open. No fitting, live/sterile input use or physical playback occurred. Roadmap status: PARTIAL.

## October 8: source-bound annotated Stash batches

[Annotated Stash batches](PULSAR_ANNOTATED_STASH_BATCHES.md) now resolve an explicit per-source scene/file/hash/view catalog into ordinary CPU generation settings. Missing/duplicate/global-view/provider and captured-source mismatches refuse. The version-5 ledger atomically requires the existing schema-13 pre-recovery validator without downgrading later storage; an actual preceding executable refuses with observed database/WAL bytes unchanged. Two distinct source/view jobs pass native generation, publication and restart without duplicate generation requests. A third wrong-hash source stops before a job; stale migration refuses, correct replacement remains paused through restart, and explicit resume completes while retaining the old catalog. All 42 checked unrounded Python estimates match exactly. Final gates pass 1,098 workspace tests (62 ignored), the native batch/migration campaign, four preceding native Stash campaigns, the default build, 284 architecture checks and 36 source guards. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-stash-batches/checkpoint.json). Actual Stash/desktop qualification, annotated packs/CUDA and placed/interval batches, mixed clocks without piecewise proof, useful learned quality, platforms, physical Handy and human/release gates remain open. No fitting, live/sterile input or physical playback occurred. Roadmap status: PARTIAL.

## October 8: owner-selected annotated CPU model packs

[Annotated model packs](PULSAR_ANNOTATED_MODEL_PACKS.md) now validate the shared named ONNX recipe during pack integrity admission and require the pinned CPU runtime/adapter for explicit owner selection. Direct pack generation requires a source-bound view, checks imported content before model capture, and retains exact pack/profile/selection documents through ordinary annotated job/receipt barriers. The shipped native campaign preserves 28 samples and all 14 intended Python estimates exactly. Actual format-11 download/import, re-export with original supplier/installed-pack/model paths unavailable, and second-clone restart pass without new active jobs. Recipe/name corruption, missing/wrong view, installed-byte drift and removed approval refuse; an actual preceding executable explicitly rejects annotated packs. Final gates pass 1,098 workspace tests (62 ignored), the native annotated pack campaign, two legacy pack campaigns, the default build, 284 architecture checks and 36 source guards. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-model-packs/checkpoint.json). Annotated Stash packs/CUDA/Preview, placed/interval batches, mixed clocks without piecewise proof, useful learned quality, actual desktop/platforms, physical Handy and human/release gates remain open. No fitting, live/sterile input or physical playback occurred. Roadmap status: PARTIAL.

## October 8: annotated Stash batches from selected packs

[Annotated Stash pack batches](PULSAR_ANNOTATED_STASH_PACK_BATCHES.md) now accept the explicitly selected CPU pack with the existing per-source scene/file/hash/view catalog. Catalog coverage is checked before annotated pack model capture; ordinary generation retains its single-source validation. Every job rechecks current owner selection and retains exact pack/profile/approval dependencies. Two distinct source/view jobs survive restart without duplicate generation requests. A third wrong-hash source stops before generation; CAS-bound replacement with a new selection revision remains paused through restart until explicit resume. All 42 checked Python estimates match exactly. Withdrawing approval during a fourth source capture prevents its future job and output. An actual preceding reader refuses while observed database/WAL bytes remain unchanged. Final gates pass 1,099 workspace tests (62 ignored), 591 Engine tests (54 ignored), eight CUDA-feature pack checks, the new native campaign, four preceding native campaigns, the default build, 284 architecture checks and 36 source guards. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-stash-pack-batches/checkpoint.json). Annotated CUDA/Preview, placed/interval batches, mixed clocks without piecewise proof, useful learned quality, actual Stash/desktop/platform qualification, physical Handy and human/release acceptance remain open. No fitting, live/sterile input or physical playback occurred. Roadmap status: PARTIAL.

## October 8: source-bound annotated CPU Preview

[Annotated Preview](PULSAR_ANNOTATED_PREVIEW.md) now accepts an explicit source-bound view and file model/profile or currently owner-selected CPU pack. It preserves both exact target images, original source-order pairing, source PTS and placement mapping, and unqualified temporal evidence through bounded worker egress. The authored CLI/local-IPC campaign passes 14 exact Python positions and 14 exact RGB pairs, nonzero source origin, asymmetric forward/reverse placements, explicit absence at first frame/cut/gap, restart, model drift and withdrawn approval. Preview adds no generation jobs or authoritative attempts and leaves the motion program unchanged. Final gates pass 1,104 workspace tests (62 ignored), 27 CUDA-feature annotated contract tests (five ignored), the new native campaign, four preceding native pack/Stash campaigns, the default build, 286 architecture checks and 36 source guards. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-preview/checkpoint.json). Annotated CUDA execution, desktop wiring/usability, placed/interval batches, mixed-clock publication without piecewise proof, broader fault/retention qualification, useful held-out model quality, installed Stash/platform/offline qualification, physical Handy and human/release acceptance remain open. No fitting, live/sterile input or physical playback occurred. Roadmap status: PARTIAL.

## October 8: annotated CPU Preview in the desktop

[Desktop annotated Preview](PULSAR_ANNOTATED_DESKTOP_PREVIEW.md) now sends explicit source-bound file-model or selected-pack requests, displays both target images and optional unqualified positions, and preserves unsupported-pair absence. Presentation binds the exact view, stride, source/placement seek and canonical model/profile/runtime identities; changing controls clears stale textures. Three ordinary tests cover changed identities/stride/seek, file-profile conflicts, pack choice retention and absence. The explicit native controller campaign admits 21 authored results including asymmetric placements/cut/gap/first-frame cases and makes 18 fresh file-model Preview calls through the same builder and SessionClient against an isolated Engine. Final gates pass 1,107 workspace tests (63 ignored), the native controller campaign, native annotated Preview and legacy selected-pack detector Preview campaigns, the default build, 286 architecture checks and 36 source guards. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-desktop-preview/checkpoint.json). Actual window rendering and human usability remain unverified. Annotated CUDA execution, placed/interval batches, mixed-clock publication without piecewise proof, broader fault/retention qualification, useful held-out quality, installed Stash/platform/offline qualification, physical Handy and human/release acceptance remain open. No fitting, live/sterile input, external Stash service or physical playback occurred. Roadmap status: PARTIAL.

## October 8: optimizer capacity controls and fixed longer-training result

[Capacity and longer-training evidence](PULSAR_REFERENCE_CAPACITY.md) now separates optimizer memorization from useful quality. A fixed 128-row optimizer-only diagnostic fits original and shuffled labels with the unchanged CPU network; all 580 development rows stay outside fitting. A separate fixed 100-epoch run retains the original group split/seed and improves optimizer MAE to 0.032632, but development MAE worsens to 0.295373 versus the old model's 0.285795 and constant baseline 0.257179. Both frozen models execute 580 current confined-worker inference runs on eight pixel/PTS/reference-identical development cases: 242 supplied reference edges each, 404 versus 331 predicted strokes, and zero matches under the unchanged scorer. Neither model is promoted. Final gates pass 36 reference Python tests, two explicitly enabled native JavaScript campaign tests, the runner build, 286 architecture checks and 36 source guards. Constructor parameters/output exactly match the preceding trainer; Rust sources and production worker binary are unchanged, so workspace/CUDA gates were not repeated. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/reference-capacity/checkpoint.json). This is consumed live training/development data; sterile inputs remain untouched. Explicit target/projection/alignment inputs and useful quality, annotated CUDA/product/batch work, actual desktop/Stash/platform/offline qualification, physical Handy and human/release gates remain open. Roadmap status: PARTIAL.

## October 8: placed and interval Stash batches

[Source-bound batch plans](PULSAR_STASH_SOURCE_PLANS.md) now reach the Engine, CLI and desktop controls. Each exact scene/file/hash plan journals an acknowledged placement before generation and freezes its publication clock with the accepted job. Migration reconciles lost placement acknowledgements and explicitly replaces or removes future placements while preserving accepted jobs. Conditional report schema 6 retains the existing schema-13 reader barrier. Native ordinary forward/reverse interval batches pass source-time publication, restart, portable import and fresh-library archive publication; annotated owner-selected pack batches preserve all 42 checked Python positions through recovery and paused source repair. Uncovered intervals still refuse publication without scan/tag effects. The preceding executable refuses the new ledgers with observed database/WAL bytes unchanged.

Final checks pass 1,121 workspace tests (63 ignored), 23 CUDA-feature Stash batch tests, four native campaigns, the default build, 288 architecture checks and 36 source guards. The [checkpoint](evidence/roadmap-2026-10-08/stash-source-plan-batches/checkpoint.json) binds 401 source records: 390 unchanged, eight changed and three added against the preceding protocol checkpoint. Intermediate fixture expectation failures and an inapplicable legacy old-reader control are retained alongside the final passing reruns; publication and eligibility guards were not relaxed. No real-media fitting, sterile access, user Stash contact or physical playback occurred in this batch.

Useful held-out model quality and real target/projection/alignment annotations, annotated CUDA execution, mixed-clock piecewise publication, actual desktop and installed Stash qualification, broader fault/retention and platform/offline qualification, physical Handy verification and human/release acceptance remain open. Roadmap status: PARTIAL.

## October 8: actual isolated Linux desktop rendering

[Native desktop startup](PULSAR_DESKTOP_RENDERING.md) now has actual 1280 × 820 window/pixel evidence under a private KWin virtual/Xwayland display, software OpenGL, DBus session and isolated Engine state. Studio controls, timeline and connected/unqualified status render. Input injection produced no observable interaction; read-only state confirms zero projects/sources/jobs, so functional window workflows and annotated-pair rendering remain unverified. The [checkpoint](evidence/roadmap-2026-10-08/desktop-rendering/checkpoint.json) retains the precise limits and terminal teardown. All 401 source records and the executable identity match the preceding passing checkpoint; no software source changed and suites were not repeated. Human usability, broader platform/offline/vendor qualification, installed Stash, useful model quality, physical Handy and release acceptance remain open. Roadmap status: PARTIAL.

## October 8: native desktop workflow and target-pair layout repair

[Actual window interaction](PULSAR_DESKTOP_PAIR_LAYOUT.md) now passes an authored Linux project/create/reopen/import/profile/view/preview flow through private KWin EIS input. It reproduced predecessor image clipping and repairs the canvas to show both targets with bounded scrollable metadata. Before/after evidence verifies default 1280 × 820 and minimum 760 × 540 windows, rounded independent model position, first-frame absence and clear/reload freshness. No generation jobs or motion revisions were created. Final checks pass 129 client tests (three ignored), the default build, 288 architecture checks and 36 source guards. The [checkpoint](evidence/roadmap-2026-10-08/desktop-pair-layout/checkpoint.json) binds 401 source records, with only desktop presentation changed. Broader desktop/human usability, installed Stash/platform/offline qualification, useful quality, annotated CUDA, mixed-clock publication, physical Handy and human/release gates remain open. Roadmap status: PARTIAL.

## October 8: actual desktop generation/export and explicit edge trim

[Desktop generation and edge trimming](PULSAR_DESKTOP_EDGE_TRIM.md) now has actual authored window evidence for generation, candidate review/commit and project-time export. The full-clip fixture exports successfully; a separate placed-source fixture establishes a real unavailable tail and refusal. New selected-axis trimming explicitly excludes only unavailable outer time through an ordinary Engine edit, preserving exact keys/evidence and internal gaps. Native trim/export, byte-identical undo/refusal, redo/export and GUI/Engine restart/reopen pass. Final checks pass 131 client tests (three ignored), the default build, 288 architecture checks and 36 source guards. The [checkpoint](evidence/roadmap-2026-10-08/desktop-generation-edge-trim/checkpoint.json) binds 401 source records with two client paths changed. Protected/internal-gap window scenarios and broader desktop/Stash/platform/offline, model quality, annotated CUDA, mixed clocks, physical Handy and human/release gates remain open. Roadmap status: PARTIAL.

## October 8: desktop internal-gap preservation and explicit unlock

The [edge-trim follow-up](PULSAR_DESKTOP_EDGE_TRIM.md) now passes actual window internal-gap preservation, export refusal without output, byte-identical Undo/Redo, and GUI/Engine restart/reopen. An authored four-key fixture loses only its two outer gaps; its internal `[300000000, 700000000)` ns gap remains. The protected placed-source fixture also passes Doctor explicit unlock followed by ordinary trim, retaining exact keys/evidence and normal automatic edit protection. Doctor now truthfully distinguishes read-only diagnostics from explicit protection changes. Final checks pass 131 client tests (three ignored), default build with existing warnings, 288 architecture checks and 36 source guards. The [checkpoint](evidence/roadmap-2026-10-08/desktop-internal-gap/checkpoint.json) binds 401 source records with only a Doctor explanation changed and verifies private-process teardown. These scoped authored native scenarios are closed; broader desktop/human usability, installed Stash, useful model quality and annotations, annotated CUDA, mixed-clock publication, fault/platform/offline, physical Handy and human/release qualification remain open. Roadmap status: PARTIAL.

## October 9: annotated CUDA file generation and portable composition

[Annotated CUDA file generation](PULSAR_ANNOTATED_CUDA_GENERATION.md) now passes a shipped CLI/Engine/supervised-worker campaign with an authored stereo convolution model: 27 runs, 324 original trace nodes all on CUDA, and maximum CPU-reference position difference 2.9802322387695312e-8. Both annotated and CUDA memberships, restart-preserved original evidence, portable clone/export equality and preceding-reader refusal without database change pass. A real missing-cuDNN-sublibrary failure was minimized to two frames and corrected only in a separate private captured dependency declaration; failing probes remain retained. Ordinary workers still reject CUDA, and independent model/receipt/trace roles and binding checks remain enforced.

Final gates pass 1,124 workspace tests (63 ignored), 600 CUDA-feature Engine tests (68 ignored), default and CUDA builds with existing warnings, both new native CUDA and existing native CPU annotated campaigns, 288 architecture checks and 36 source guards. The [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-cuda-generation/checkpoint.json) binds 405 source records: 392 preceding unchanged, nine changed, one added and three existing auxiliary research files newly captured by inventory audit. Private processes are terminal and worker custody is empty. Annotated CUDA Preview/batches remain explicitly unsupported; selected annotated CUDA packs/desktop and sustained performance need verification. Useful model quality/annotations, mixed clocks, installed Stash, broader fault/platform/offline, physical Handy and human/release gates remain open. No fitting, live/sterile input, user service contact or physical playback occurred. Roadmap status: PARTIAL.

## October 9: annotated CUDA Stash file-model batches

[Annotated CUDA Stash batches](PULSAR_ANNOTATED_CUDA_STASH_BATCHES.md) now validate owner policy, pinned runtime/device and offline reservation before discovery, while preserving supervised capture and strict worker evidence. The authored native campaign passes two distinct source/view bindings through restart and a third explicit hash repair through paused migration and separate resume. Each of three attempts retains 27 successful model runs and 324 CUDA-only computation nodes; all 42 rounded positions match Python and the maximum unrounded difference is 2.9802322387695312e-8. Missing owner, runtime mismatch and offline ceiling refuse before discovery/ledger/jobs. Exact CUDA evidence survives restart; native CPU regression also passes.

Final verification passes 1,125 workspace tests (63 ignored), 601 CUDA Engine tests (68 ignored), the CUDA build, 288 architecture checks and 36 source guards. The initial workspace run failed an existing immediate worker-fence lease reacquisition test; its isolated recheck and full repeat passed without changes. That intermittence remains unresolved and both outcomes are retained. The [checkpoint](evidence/roadmap-2026-10-09/annotated-cuda-stash-batches/checkpoint.json) binds 405 source records, with two source paths changed and 403 unchanged. Private processes are terminal and custody empty. Selected annotated CUDA packs, placed/interval CUDA batch verification, CUDA preview, useful quality/real annotations, mixed clocks, sustained performance, installed Stash, broader fault/platform/offline, physical Handy and human/release gates remain open. No fitting, live/sterile access, user service contact or physical playback occurred. Roadmap status: PARTIAL.

## October 9: selected annotated CUDA packs and placed/interval batches

[Selected annotated CUDA pack batches](PULSAR_ANNOTATED_CUDA_PACK_BATCHES.md) now have native authored evidence for both unplaced and per-source placed/interval workflows. The existing Engine paths pass without production changes. Two CUDA campaigns and their two CPU regressions preserve exact pack/profile/approval dependencies, per-source view bindings, restart, paused explicit migration and separate resume. Withdrawal before a later generation creates no fourth job/output and remains paused after restart. Six completed CUDA attempts each retain 27 runs and 324 CUDA-only trace nodes; all 84 checked rounded positions match Python with maximum unrounded difference 2.9802322387695312e-8. An independent fraction check validates all 84 placed sample times, including reverse and subnanosecond mappings.

The [checkpoint](evidence/roadmap-2026-10-09/annotated-cuda-pack-batches/checkpoint.json) binds 406 source records: 403 preceding unchanged, two verification scripts changed and one shared fixture helper added. The production sources and executable remain byte-identical to the preceding checkpoint; Cargo suites were not repeated. Their preceding 1,125 workspace/601 CUDA Engine results and unresolved initial lease-test intermittence remain explicitly inherited. Current syntax, 288 architecture checks and 36 source guards pass. Private processes are terminal and worker custody empty. File-model CUDA placed/interval native verification, annotated CUDA preview/desktop, sustained performance, useful quality/real annotations, mixed clocks, installed Stash, broader fault/platform/offline, physical Handy and human/release gates remain open. No fitting, live/sterile input, user service contact or physical playback occurred. Roadmap status: PARTIAL.

## October 9: annotated CUDA file-model placement and intervals

[File-model annotated CUDA batches](PULSAR_ANNOTATED_CUDA_STASH_BATCHES.md) now pass authored native per-source forward/reverse placement, explicit intervals and source-time publication, closing the remaining batch combination without production changes. Two original sources and an explicitly repaired third source retain three clock correspondence proofs and original 28-action timestamps. An independent fraction check validates all 84 sample project times, including reverse subnanosecond mappings. Each CUDA attempt retains 27 real runs and 324 CUDA-only trace nodes; all 42 checked rounded positions match Python with maximum unrounded difference 2.9802322387695312e-8. Owner/runtime/offline admission refusals, restart, exact original evidence and paused migration/separate resume pass. Placed and unplaced CPU regressions pass.

The [checkpoint](evidence/roadmap-2026-10-09/annotated-cuda-placed-file-batches/checkpoint.json) binds 406 source records with one verification script changed and 405 unchanged. Production sources and executable remain identical; Cargo gates and their earlier lease-test intermittence are explicitly inherited, not rerun or repaired. Current syntax, 288 architecture checks and 36 source guards pass. Processes are terminal and custody empty. CUDA preview is next: first-frame/unsupported-pair no-estimate results need an explicit validated no-inference contract alongside strict positive-run evidence. CUDA desktop, useful quality/real annotations, mixed clocks, sustained performance, installed Stash, broader fault/platform/offline, physical Handy and human/release gates remain open. No fitting, live/sterile input, user service contact or physical playback occurred. Roadmap status: PARTIAL.

## October 9: annotated CUDA preview and explicit no-inference evidence

[Annotated CUDA preview](PULSAR_ANNOTATED_CUDA_PREVIEW.md) now passes 21 authored native previews: 18 supported pairs each retain one real CUDA run and 12 CUDA-only computation nodes; first-frame, long-gap and scene-cut cases return no estimate with distinct zero-run initialization-only evidence. Parent validation independently checks original pixels and exact request/context/model/profile bindings before accepting zero runs. Generation and historical/portable positive-run verification remain strict. Exact RGB pairs, source-PTS and forward/reverse placement, restart, model drift and approval withdrawal pass without new authoritative jobs/attempts or master-motion edits. CPU preview and CUDA generation/restart/portable clone/export regressions pass. The initial offline-profile preview-budget refusal is retained alongside the corrected explicit interactive-profile test.

The [checkpoint](evidence/roadmap-2026-10-09/annotated-cuda-preview/checkpoint.json) binds 406 source records, eight changed paths and 398 unchanged. Final gates pass 1,129 workspace tests (63 ignored), 604 CUDA Engine tests (68 ignored), default/CUDA builds with existing warnings, 288 architecture checks and 36 source guards. Current suites pass without retry; prior lease-test intermittence remains unresolved. Private campaigns are terminal and custody empty. Actual desktop CUDA interactions and sustained performance are next. Useful model quality/real annotations, mixed clocks, installed Stash, broader fault/platform/offline, physical Handy and human/release gates remain open. No training, live/sterile input, user service contact or physical actuation occurred. Roadmap status: PARTIAL.

## October 9: actual Linux CUDA preview window and provider wording

[Desktop CUDA preview](PULSAR_DESKTOP_CUDA_PREVIEW.md) now has authored native window evidence for project creation/import, explicit CUDA profile and target-view loading, restart/reopen, first-frame absence, supported pair rendering and target clearing. A found presentation defect labeled CUDA output a CPU estimate; two client paths now use CPU/CUDA profile wording, an unqualified estimate and a pinned declared runtime. The rebuilt window displays exact source ordinals 6/7 at 0.875 seconds; the original receipt has one real run and 12 CUDA-only computation nodes. Two first-frame receipts retain zero runs and initialization-only evidence. Target clearing removes both textures and the old position. Master revision stays zero, with no authoritative jobs/attempts.

The [checkpoint](evidence/roadmap-2026-10-09/desktop-cuda-preview/checkpoint.json) binds 406 source records, two changed and 404 unchanged, the rebuilt executable and original evidence/screenshots. Current gates pass 131 client tests (three ignored), CUDA build with existing warnings, 288 architecture checks and 36 guards; preceding workspace/CUDA Engine suites and unresolved lease intermittence are explicitly inherited. Private GUI/Engine/compositor are terminal and custody empty. Desktop CUDA generation/commit/export and selected-pack window verification remain open. Observed waits do not establish sustained latency or the 200 ms requirement. Useful quality/real annotations, mixed clocks, installed Stash, broader fault/platform/offline, physical Handy and human/release gates remain open. No training, live/sterile input, user service contact or physical actuation occurred. Roadmap status: PARTIAL.

## October 9: actual Linux CUDA generation, commit, export and restart

[Desktop CUDA generation](PULSAR_DESKTOP_CUDA_GENERATION.md) now passes an authored file-model workflow through actual window controls: fresh project/import, explicit CUDA profile and target-view loading, generation, candidate inspection, commit, export, Undo/Redo and GUI/Engine restart/reopen. Generation leaves master revision zero until explicit commit. The original bound attempt has 27 runs and 324 CUDA-only computation nodes; all 28 action times and rounded exported positions match the candidate, with maximum Python-reference difference 2.9802322387695312e-8. Commit reaches revision one, Undo restores the empty master at revision two and Redo restores the exact motion at revision three. Redo and restart exports are byte-identical, original CUDA evidence unchanged, and no additional job is created.

The [checkpoint](evidence/roadmap-2026-10-09/desktop-cuda-generation/checkpoint.json) binds 406 unchanged source records and an identical executable, with original request/receipts/traces and actual screenshots. Preceding client/build/architecture/guard gates and unresolved lease-test intermittence are inherited; no Cargo rerun is claimed. Private GUI/Engine/compositor are terminal and custody empty. Selected-pack and placed/interval desktop combinations, sustained performance, useful quality/real annotations, mixed clocks, installed Stash, broader fault/platform/offline, physical Handy and human/release gates remain open. No training, live/sterile input, user service contact or physical actuation occurred. Roadmap status: PARTIAL.

## October 9: selected CUDA pack desktop workflow and withdrawn authority

[Selected CUDA pack desktop verification](PULSAR_DESKTOP_CUDA_PACK.md) now passes actual authored window import/inert integrity, separate approval, target-view loading, first-frame absence, exact supported-pair display, generation, candidate inspection, commit and export. The request binds exact manifest/profile/approval dependencies. The completed generation retains 27 runs and 324 CUDA-only computation nodes; all 28 exported times and rounded positions match. The supported preview retains one run and 12 CUDA-only nodes. No master change occurs before explicit commit.

Desktop deactivation reaches selection revision two with no approval. Replaying the original selection receipt returns historical revision one without restoring current authority. Future generation visibly refuses, preview clears, no new job/receipt is created and committed export stays byte-identical. GUI/Engine restart preserves the inactive selection, refusal, exact export and original evidence. The [checkpoint](evidence/roadmap-2026-10-09/desktop-cuda-pack/checkpoint.json) binds 406 unchanged source records and an identical executable; preceding gates and unresolved lease-test intermittence are explicitly inherited. Private processes are terminal and custody empty. Placed/interval desktop CUDA combinations and sustained performance are next; useful quality/real annotations, mixed clocks, installed Stash, broader fault/platform/offline, physical Handy and human/release gates remain open. No fitting, live/sterile input, user service contact or physical actuation occurred. Roadmap status: PARTIAL.

## Independent audio clock rates, 2026-10-09

[Declared-rate correspondence](PULSAR_INDEPENDENT_AUDIO_RATE.md) adds an optional positive rational audio-seconds-per-video-second rate, exact affine mapping and desktop integer text controls. Legacy unit-rate JSON remains unchanged; invalid rates refuse before source import/admission. Current source suites pass 99 protocol, 631 default Engine, 633 CUDA Engine and 137 client tests, with 0/55/69/3 ignored respectively. Offline CUDA release/bundle, 289 architecture checks and 36 guards pass. Two authored final-executable campaigns preserve restart and exact alignment bytes through clone/re-export/second clone; an independent Fraction oracle verifies 48 clock/window samples. Source 417 records has eight changed paths and 409 unchanged. Earlier compile/harness failures are retained and corrected final gates pass. Measured alignment, nonlinear clocks, rendered rate-control QA and all model/media/Stash/Handy/platform/performance/human/release gates remain open. Full roadmap remains PARTIAL.

## Live Stash generation clocks, 2026-10-09

[Generation clock correspondence](PULSAR_STASH_GENERATION_CLOCK_GUARD.md) repairs a native counterexample where two placements on one captured source shared an empty retiming history and were published under one origin clock. An interior 250 ms project point derived from 500 ms source time was incorrectly labelled 250 ms. Bounded typed origin checks now preserve equal-clock independent jobs and refuse differing clocks without creating a sibling script. The native regression changes red to green; the existing publication/retiming/restart/portable-clone campaign passes. Current default/CUDA Engine suites pass 632/634 (55/69 ignored), offline CUDA release/bundle and 289 architecture/36 guard checks pass. Source 418 records has one changed production path, one new test and 416 unchanged. Fixture setup/evidence-capture negatives remain explicit; nine owned Engines are terminal. Installed Stash has no observed listener on configured port 9999. Explicit piecewise publication and all model/media/Stash/Handy/platform/performance/human/release gates remain open. Full roadmap remains PARTIAL.


## Actual Stash authored compatibility, 2026-10-09

[Real Stash checkpoint](PULSAR_REAL_STASH_COMPATIBILITY.md) exercises the production CLI/Engine against an isolated official v0.31.1 server and three authored videos. Discovery excludes the preexisting scripted scene; the two missing scenes complete capture, generation, commit, no-clobber publication, real scan attachment and tag creation/ADD. Both scan receipts are verified_linked; independent GraphQL observes interactive scenes and one exact shared tag. Fresh Engine status is identical, submission replay preserves its original acknowledgement without changing finished status, and rediscovery/new batch select zero items. New script hashes match receipts; original inputs/script and user config/database are unchanged. All 418 source records are unchanged; no new build or broader suite is claimed. Harness state/replay/CLI-syntax negatives are retained and corrected. Owned units are terminal with empty custody. Anonymous authored v0.31.1 compatibility now has actual-server evidence; authenticated/user-library/desktop/version-matrix compatibility and all model/media/live/Handy/platform/human/release gates remain open. Full roadmap PARTIAL.


## Explicit selected-clock arithmetic, 2026-10-09

[Selected clocks](PULSAR_SELECTED_SOURCE_CLOCKS.md) add bounded exact per-action carrier selection in core, preserving merge-boundary ownership supplied by the caller and keeping output carrier indices parallel through source reordering. Six new tests cover different clocks, preserved boundaries/interior frame clocks, correct-source collisions, reverse/fractional equivalence, authored evidence and bounded/refused inputs. Direct current-source builds pass 43 core, 632 default Engine and 634 CUDA Engine tests (0/55/69 ignored), plus 291 architecture checks and 36 source guards. Initial dependency/compile harness setup failures are retained; corrected builds pass. Source 419 records has two existing core changes, one new module and 416 unchanged existing records. This is the arithmetic prerequisite for Engine ancestry verification and versioned durable mixed-clock proofs; existing publication refusal remains enabled. No Cargo workspace/release build, current runtime candidate or full publication workflow is claimed. Full roadmap PARTIAL.


## Native clock origins and exact read admission, 2026-10-09

[Native origin checkpoint](PULSAR_NATIVE_CLOCK_ORIGINS.md) centralizes bounded project/source/job/candidate/receipt loading for existing native publication, validates actual original CAS motion and retains metadata/plan/raw-program memory reservations. Actual verifier counterexamples consume 75,849 bytes after an observed 10,313-byte receipt and 66,094 after a declared 558-byte program; both become green with exact read bounds. Source catalogue mismatch, oversized metadata, low-memory and corrupted/replaced CAS cases refuse. Direct default/CUDA Engine suites pass 632/634 (55/69 ignored), offline locked CUDA release and final source-consistency build pass, and the rebuilt CLI passes both native clock/publication/retiming/restart/portable-clone campaigns without skips. Architecture/guards pass 292/36. Source 420 records has three existing changes, one new Authority helper and 416 unchanged existing records. Intermediate module/import/borrow/read-only fixture failures are retained and corrected. Both native fences are empty; private state/build scratch is removed after custody checks. Mixed-clock action ancestry and versioned durable proofs remain pending; publication refusal and all broader qualification gates remain in force. Full roadmap PARTIAL.


## Native contributor receipt verification — 2026-10-09

[Contributor origin verification](PULSAR_CONTRIBUTOR_CLOCK_VERIFICATION.md) closes an actual CLI defect: the predecessor published an equal-clock merged script despite a corrupted contributor receipt. The bounded ancestry walk now verifies each contributing native origin and sample-plan arithmetic. Corrupt/missing receipts refuse before sibling creation; restoration permits valid publication. Default/CUDA Engine suites, offline release, architecture guards and both native CLI regressions pass. Per-action clock derivation and durable mixed-clock proofs remain pending; roadmap status remains PARTIAL.


## Exact merge action origins — 2026-10-09

[Exact merge action origins](PULSAR_EXACT_MERGE_ACTION_ORIGINS.md) adds an optional collector to the production merge kernel, retaining each output action’s exact Current/Candidate input and original index under the actual protection/write mask. Equal-valued boundaries remain Current. Core composition checks the correct source frame for an interior contributor and refuses a source-time collision. All core unit/integration targets and default/CUDA Engine regression suites pass. Engine ancestry composition, durable mixed-clock proofs and portable closure remain pending; roadmap status remains PARTIAL.


## Native authored-edit clock origins — 2026-10-09

[Native edit origin verification](PULSAR_NATIVE_EDIT_CLOCK_ORIGINS.md) closes a reproduced CLI defect: a corrupted authored-edit receipt previously still permitted publication. The ancestry walk now verifies receipt/upload/CAS identities and replays values-only reconciliation, including exact authored/inherited intervals. Corrupt/missing evidence refuses and restoration permits valid publication. Source-bound release and native regressions pass; an earlier worker-fence lease failure and incomplete serial attempt are retained, with concurrency root cause unresolved. Per-action ancestry composition and durable mixed-clock proofs remain pending; roadmap status remains PARTIAL.


Current integration update: authenticated asynchronous native preparation, original-request resolution, status, cancellation/revocation and retained-use release are now implemented with CLI and desktop controls. See [preparation control](PULSAR_NATIVE_PREPARATION_CONTROL.md) and its checkpoint for current verification. Earlier statements that these surfaces remain unwired describe the preceding journal increment. Native execution, rendered desktop evidence and the full roadmap remain open.


Production transport qualification update: the new SessionClient native campaign fails at its first 180-second preparation deadline after successful Start/replay/resolution. Live GPU inference and cleanup receipt are not reached. See [native preparation process](PULSAR_NATIVE_PREPARATION_PROCESS.md); the existing cold post-Finish case remains unchanged. Roadmap status remains PARTIAL.


Subsequent native verification: two unchanged-source production SessionClient campaigns pass retention, replay/resolution, shared-use release, three GPU estimates, Finish and checked cleanup. I/O profiling still finds long writes and fsync stalls; the earlier deadline failure and load-independent reliability remain open. See [native preparation I/O profile](PULSAR_NATIVE_PREPARATION_IO_PROFILE.md). No useful model/media quality or physical-device qualification is implied.


### 2026-10-10: durable native preparation client recovery

CLI and desktop save bounded private Start metadata before dispatch and restore it after reopening. Recovery resolves the original request with current authority and revision while preserving original epoch, revision and options. Exact duplicate intent is permitted; conflicting IDs, unsafe storage, changed credential/endpoint bindings and capacity overflow refuse before Start. The panel lists saved identities without automatically starting or resolving work. Explicit local forgetting does not cancel or release engine operations. Replies are checked against owner, project, request/operation and frozen intent. See [client recovery](PULSAR_NATIVE_PREPARATION_CLIENT_RECOVERY.md).

This increment does not close full-workspace concurrency/lint/format, OS-death/power-loss, rendered desktop, native reliability/cold post-Finish, useful annotated training/media, live Stash, Handy 2 Pro firmware/encrypted keys/physical stop and synchronization, platform or release gates. Full roadmap PARTIAL; goal active.


### 2026-10-10: engine quality and renewed process gates

Engine library lint errors fall from 46 to 25 after behavior-preserving type/syntax cleanup and removal of unused duplicate metadata. All 703 engine tests and 192 client tests pass in completed package commands; ignored qualification cases remain separate. Large-motion and portable-desktop process suites pass in the full workspace run, which then fails at package-export restart with a transport I/O error. Two isolated probes fail startup; the contextual probe identifies initial fixture startup, with a live engine waiting in the filesystem journal before database creation. Root cause and repair of the original transport failure remain unproven. Strict engine Clippy still fails with 25 library/72 test-target errors, global formatting fails, and 350 architecture checks/36 source guards pass. See [engine quality follow-up](PULSAR_ENGINE_QUALITY_FOLLOWUP.md). Full roadmap PARTIAL; goal active.


### 2026-10-10: native capture limits and custody interface

Named capture limits and staging custody replace positional resource/custody pairs without changing the copying implementation, admission checks or original inherited clock. The expiry/cancellation regression now covers a 1-hour preparation budget as well as 1 second. All 703 engine tests pass; separate explicitly ignored confined-worker cases pass inherited-lease retirement and storage ownership/cleanup across engine drop and same-scope restart. Engine strict lint remains FAIL at 22 library/69 test-target errors; 350 architecture checks and 36 source guards pass. An unchanged startup sync trace passes initialization and does not reproduce or repair the earlier journal stall. See [native capture interface](PULSAR_NATIVE_CAPTURE_INTERFACE.md). Full roadmap PARTIAL; goal active.


### 2026-10-10: assistant memory handoff

[Owned generation handoff](PULSAR_ASSISTANT_MEMORY_HANDOFF.md) adds authenticated pause/admission/resume orchestration for memory contention. Current authority, immutable checkpoints and dependencies gate the pause; signals follow transaction commit, and actual custody release precedes the assistant reservation loan. Normal ResumeJob retains the job ID and starts a new attempt. Intentional pauses preserve the failure retry budget while retained history stays bounded. Worker cleanup checks exact cancellation-handle identity. Annotated reopen and withdrawal recognize paused attempt history.

Source-bound evidence and final checks are recorded in the [checkpoint](evidence/roadmap-2026-10-10/assistant-memory-handoff/checkpoint.json). Authored fixtures do not close native learned-model handoff, GPU/live/desktop continuity, ordinary-storage reliability, useful annotations/media quality, installed Stash/offline/platform, Handy 2 Pro credentials/firmware/physical stop and synchronization, human or release qualification. No annotations or physical results were fabricated. Full roadmap PARTIAL; goal active.


### 2026-10-10: assistant journal capture admission

[Bounded assistant recovery](PULSAR_ASSISTANT_JOURNAL_ADMISSION.md) checks SQLite cell types/bytes before Rust decoding, stored/report identity agreement, 128-row catalogue capacity and bounded JSON publication. Normal lifecycle and input-checkpoint handoff remain authoritative. Damaged cells refuse explicit recovery without truncation or cancellation signalling. Source-bound checks are retained in the [checkpoint](evidence/roadmap-2026-10-10/assistant-journal-admission/checkpoint.json).

This software increment does not close ordinary-storage reliability, whole-application/SQLite peak memory, native learned-model handoff/live continuity, model/media/annotations, installed Stash/offline/default delivery, platform, Handy 2 Pro or human/release qualification. Full roadmap PARTIAL; goal active.


### 2026-10-11: automatic assistant capacity recovery

[Capacity-release recovery](PULSAR_ASSISTANT_CAPACITY_RECOVERY.md) retries pending assistant completion and owned generation resume after unrelated reservations retire, without client polling or repeated inference. The weak singleton waits on bounded coalesced external-release notifications; its own releases do not cause retry loops. Ordinary authority, checkpoint and native/GPU admission remain in force. Source-bound checks and separate native learned-proposal evidence are retained in the [checkpoint](evidence/roadmap-2026-10-11/assistant-capacity-recovery/checkpoint.json).

Full learned offline/live contention, ordinary storage reliability, whole-application performance, useful annotations/model/media, installed Stash/offline/default/platform, Handy 2 Pro and human/release qualification remain open. Full roadmap PARTIAL; goal active.


### 2026-10-11: broad Engine/client regression and milestone ledger

The missing detailed artifact named by the major roadmap is now the [M1–M8 gate ledger](PULSAR_MILESTONES.md). It preserves required modality, editing/clock/provenance, model/assistant/memory, six-axis/device, Core/Complete/offline, vendor/platform, quality/speed/human-effort and release scope. No milestone is marked complete. Historical scoped evidence is bound to its original subjects; current user libraries and later source versions are not silently inherited.

The unchanged 370-source subject passes complete private tmpfs default Engine tests (761 passed/59 ignored, 24.50 seconds), all-feature Engine tests (775 passed/84 ignored, 32.00 seconds), and all-feature client tests (218 passed/four ignored, 7.71 seconds). The ordinary-storage complete default Engine run TIMEOUT124 at 360 seconds during assistant generation receipt coverage without a completed suite verdict. The process snapshot showed the main test process waiting on a futex; that observation does not establish the slow fixture's cause. Earlier ordinary storage/concurrency failures remain unresolved; a size-capped private mount changes the environment and cannot repair them. Native ignored cases, full workspace/process suites and model/media/device/platform/human/release acceptance remain separate. [Exact regression checkpoint](evidence/roadmap-2026-10-11/engine-regression/checkpoint.json). Full roadmap PARTIAL; goal active.


Subsequent [ordinary-storage diagnosis](PULSAR_STORAGE_SYNCHRONIZATION_DIAGNOSIS.md) reproduces an 11.64-second fsync stall outside Pulsar/SQLite, while the previously stalled exact fixture passes in 0.29 seconds. A serial default suite still times out at180 seconds; journal-wait thread snapshots and individual sync timings are retained. No production repair or full-suite verdict is claimed. All370 source/config hashes and the default binary remain unchanged; full roadmap PARTIAL.


### 2026-10-11: portable approved assistant assets

[Portable assistant policy](PULSAR_PORTABLE_ASSISTANT_POLICY.md) adds schema-2 runtime/model paths relative to the selected private owner policy, with traversal/symlink/missing-path refusals and unchanged exact-byte capture, approval and admission. Relocation and refusal tests pass on ordinary storage; final default Engine763/zero failures/59ignored passes on private tmpfs. Both strict workspace lint configurations, formatting, architecture376, guards36 and final all-feature binaries PASS. Actual Core/Complete artifacts, installed defaults, useful quality, storage reliability, native/platform/device and human/release gates remain open. Full roadmap PARTIAL; goal active.


Runtime assembly follow-up: [dedicated worker packaging repair](PULSAR_LINUX_RUNTIME_BUNDLE.md#dedicated-worker-assembly-repair--2026-10-11) makes the current Linux candidate assembler include the required sibling worker. Identical-input predecessor tests FAIL2 and corrected tests PASS2 in private tmpfs; actual Core/Complete delivery and ordinary-storage reliability remain open.


### 2026-10-11: first-party offline assistant host setup

[Local host setup](PULSAR_ASSISTANT_HOST_SETUP.md) measures selected local asset identities, validates existing approval/deadline/memory admission and writes a new private policy before Engine startup. No RPC, network, inference or project effect is added. Native CLI metadata fixtures and full selected Engine/client libraries PASS on private tmpfs; ordinary policy testing remains incomplete, and no storage repair is claimed. Both strict workspace lint configurations, formatting, architecture376, guards36 and final all-feature binaries PASS. Core/Complete delivery, useful model/media, native/platform/device and human/release gates remain open; goal active.


### 2026-10-11: retained Linux assistant bundle candidate

[Static assistant packaging](PULSAR_STATIC_ASSISTANT_BUNDLE.md) adds optional pinned runtime/model/license assembly and retains an actual twelve-file candidate on ordinary storage. Final relocated offline host-setup tests PASS2, existing worker-bundle tests PASS2, and the pinned static runtime passes six authored worker proposals and seven refusal checks. All retained file identities match the final tested manifest. The actual ELF requires x86-64-v3 despite conservative compilation flags; older CPU/OS support and transitive static licensing are unqualified. No default model is activated. Core/Complete generation packs, useful annotations/model/media, storage/native/platform/device and human/release gates remain open. Full roadmap PARTIAL; goal active.


### 2026-10-11: bundled CPU runtime discovery and actual relocated generation

[CPU runtime delivery](PULSAR_BUNDLED_CPU_RUNTIME.md) removes the manual runtime-path requirement for a package-supplied ONNX Runtime while preserving explicit overrides and confined exact-byte execution. A preceding copied binary reports runtime:null with the same runtime file; the updated candidate executes authored direct-profile and explicitly owner-selected offline-pack generation after relocation without media/runtime overrides. Native exports agree byte-for-byte and final worker custody is empty. The retained fifteen-file candidate remains unqualified; useful generation assets/defaults, clean-machine/system dependency/platform/license coverage, extraction/update/data relocation, storage/live/model/media/device and human/release gates remain open. Full roadmap PARTIAL; goal active.


### 2026-10-11: first-party offline runtime candidate installation

[Offline runtime installation](PULSAR_OFFLINE_RUNTIME_INSTALLATION.md) adds bounded exact-byte capture, private staging, durable file/directory synchronization and atomic no-overwrite publication before Engine configuration. Existing destinations remain untouched, and process-death stages are retained for explicit inspection. The source-bound checkpoint records final library/lint/native results and unresolved ordinary-storage/harness outcomes. Candidate import does not close signed delivery, first-launch extraction, orphan management/update/rollback, generation defaults/model/media, OS/vendor/device or human/release gates. Full roadmap PARTIAL; goal active.


### 2026-10-11: explicit runtime installation stage recovery

[Stage recovery](PULSAR_RUNTIME_STAGE_RECOVERY.md) adds a private locked receipt, read-only inspection and explicit exact-identity discard through the trusted host before Engine configuration. Native active-writer refusal, SIGKILL inspection, stale/unknown-entry preservation, exact authorized discard and umask0777 retry PASS. Default Engine775/client221 and both strict workspace lint configurations pass; the actual updated candidate is retained after ordinary installation. Legacy unmarked/damaged stages, exhaustive storage/external mutation/faults, extraction/update/rollback, useful model/default/edition/platform/media/device and human/release gates remain open. Full roadmap PARTIAL; goal active.


### 2026-10-11: single-file offline runtime candidate delivery

[Offline runtime archive](PULSAR_OFFLINE_RUNTIME_ARCHIVE.md) adds versioned uncompressed packaging and extraction before Engine configuration. Bounded manifests and streaming exact-byte integrity retain existing private stage/receipt/publication/recovery. The source-bound checkpoint records full selected library/lint and actual fifteen-payload native results. This closes the directory-only candidate transport limitation; first-launch embedding, opaque archive-temp recovery, updates/rollback, actual edition assets/defaults, platform/storage/license/model/media/device and human/release gates remain open. Full roadmap PARTIAL; goal active.


### 2026-10-11: persistent first-use runtime custody and actual AppImage delivery

[AppImage runtime custody](PULSAR_APPIMAGE_RUNTIME_CUSTODY.md) adds private manifest-addressed installation, exact cache verification and trusted-host execution before Engine startup. The AppImage builder consumes pinned official tools and supplied candidate bytes without network fetch, retaining original manifest/payload identities. The checkpoint records default libraries, lint/build and actual transient-client/continuing-Engine/later-worker outcomes. Actual Core/Complete models/defaults, clean-machine/FUSE/platform/vendor/license/signing/filesystem/update/rollback and model/media/device/human release gates remain open. Full roadmap PARTIAL; goal active.


### 2026-10-11: AppImage musl cleanup correction: [downstream cleanup correction](PULSAR_APPIMAGE_CLEANUP.md) replaces the upstream zero-descriptor nftw call, retains exact modified-runtime source/build identity and restores the automatic cleanup requirement in the actual AppImage campaign. Its checkpoint distinguishes preceding failures, source regression and current artifact results. Interrupted/concurrent extraction, orphan handling, broad platform/signing/license/model/edition/media/device and human release gates remain open; no milestone is closed.


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


### 2026-10-11: native CPU assistant/generation contention

[Native CPU handoff](PULSAR_NATIVE_ASSISTANT_HANDOFF.md) exercises permitted pinned YOLO11 generation and Qwen3 inference through the actual worker under incompatible aggregate admission claims. Passive journal observation confirms automatic same-job completion, intentionally paused history, both exact checkpoints, unchanged logical input basis and master motion. No status poll or manual resume is used. The new explicitly ignored native case passes separately; default Engine 813 PASS/62 ignored, both strict lints, format, 398 architecture checks and 36 guards PASS.

This test-only increment does not prove completed generation inference before the pause, partial-progress recovery, aggregate process memory, GPU/live/desktop continuity, ordinary-storage reliability, useful trained models or annotated media, platform/Handy/human/release acceptance. Full roadmap remains PARTIAL. See [ADR B3-125](../research/phase-b/ADR-B3-125-native-assistant-generation-handoff.md) and the [source-bound checkpoint](evidence/roadmap-2026-10-11/native-assistant-handoff/checkpoint.json).


### 2026-10-11: distinct bounded visual tracking tiers

[Visual tracking plans](PULSAR_VISUAL_TRACKING_TIERS.md) replace identical Fast/Default work and worker refusal of visual Maximum with explicit bounded search/grid plans. Default retains existing tracking; Fast reduces point work and skips pyramid recovery; Maximum increases initial target density and search radius while preserving independent evidence checks. Candidate-bound receipts disclose the effective plan and actual sample work. A confined pinned YOLO11 CPU case executes all three plans with nonempty detections/flow on repeated public bus frames and leaves master motion/revision unchanged. Static zero relative motion remains zero for every plan.

This does not complete M3: useful pinned defaults, all-input pipelines, paired natural-media quality/speed/resource qualification, GPU vendors, offline editions, physical and human/release acceptance remain open. See [ADR B3-126](../research/phase-b/ADR-B3-126-bounded-visual-tracking-tiers.md) and the [source-bound checkpoint](evidence/roadmap-2026-10-11/visual-tracking-tiers/checkpoint.json). Full roadmap PARTIAL; goal active.


### 2026-10-11: desktop and Stash tier admission

[Shared executable tier admission](PULSAR_TIER_ADMISSION.md) exposes visual Maximum in desktop preparation and Stash batch/migration, validates the resolved model before fresh job/ledger publication, and refuses incompatible requests without changing their chosen tier or replacing the retained migration ledger. Worker execution checks the same support rule. Structural archival validation and serialized settings remain unchanged. Selected-pack and owner/current-revision controls remain independent. See [ADR B3-127](../research/phase-b/ADR-B3-127-shared-tier-admission.md) and the [source-bound checkpoint](evidence/roadmap-2026-10-11/tier-admission/checkpoint.json).

This does not qualify rendered GUI, installed external Stash, useful/default/all-input models, natural-media quality/speed, GPU vendors, platform, physical Handy or human/release acceptance. Full roadmap PARTIAL; goal active.


Learned CUDA execution update: [native visual tier evidence](PULSAR_LEARNED_CUDA_TIERS.md) verifies the pinned YOLO11 Fast plan against original CUDA artifacts and candidate lineage on the owner's RTX 4090. The missing convolution cuDNN components are explicitly pinned in the test policy; All three rebuilt scoped native checks pass, each with four learned inferences, original CUDA artifacts, candidate binding, unchanged master and terminal retirement. This is not model-quality, default-model, speed/resource, vendor, media, physical or human/release qualification. Full roadmap PARTIAL; goal active.


Profile RAM admission update: [owner-bounded model claims](PULSAR_PROFILE_RAM_ADMISSION.md) replaces the fixed generation/preview RAM claim with the declared minimum above the existing floor, checks current owner capacity, and reserves the same bytes. Resumed budgets stay exact; CPU/output/time limits and qualification authority remain independent. Larger declared learned-model CPU requests pass; native CUDA and completed gates are separately recorded. Full roadmap PARTIAL; goal active.


Aggregate CPU admission update: [owner-bounded worker quotas](PULSAR_CPU_ADMISSION.md) adds CPU claims to the shared RAM/job/GPU pool and checks cgroup/assistant-loan consistency. Generation, preview, live, native capture, WASM and assistant worker paths use their declared quotas, retained through existing custody retirement. Impossible owner claims refuse before pausing generation. Completed checks and scoped native CPU-only contention are recorded separately; model/media/platform/device/human qualification remains open. Full roadmap PARTIAL; goal active.


Bundled media fixture update: [orientation and source-clock coverage](PULSAR_BUNDLED_MEDIA_FIXTURES.md) removes the regression's optional libx264 encoding dependency while retaining separate authored H.264 decoder coverage. MPEG-4 and H.264 cases require actual rotation metadata, unchanged decoded pixels, exact frame counts and independently expected source times. Completed bundled/system tool checks are recorded separately; full media/model/platform/device/human qualification remains open. Full roadmap PARTIAL; goal active.


## Semantic image runtime candidate — 2026-10-11

[Native pixel controls](PULSAR_SEMANTIC_IMAGE_RUNTIME.md) pass two authored circle/square cases through a static, confined CPU vision runtime with whole-body schema JSON. Current image generation remains prompt-conditioned; production vision/profile/custody/approval integration and task quality remain open. No training, real-media, GUI, device or release qualification is claimed. Roadmap: PARTIAL.


## Bound semantic image worker — 2026-10-11

[Image assistant worker foundation](PULSAR_SEMANTIC_IMAGE_WORKER.md) binds exact RGB/projector identities through capture, aggregate admission, confinement, strict response checks and retirement. Final default/all-feature Engine suites pass 835/849 tests, protocol 126, and five actual native parent tests including seven new scripted vision fault cases and existing learned text regression. Both image-to-pattern intent controls FAIL; the candidate remains unqualified. Authenticated vision profile/source/preprocessing/report/approval integration and client presentation remain open; existing image generation stays explicitly prompt-conditioned. Roadmap: PARTIAL.


## Desktop atomic neutral bundle — 2026-10-12

The [desktop checkpoint](evidence/roadmap-2026-10-12/desktop-neutral-bundle/checkpoint.json) connects the existing atomic helper to desktop export/resume actions with exact selected revision, retained typed failure paths, explicit uncertain-outcome disposition and local stop ownership. Drawn controls and an actual native transfer pass; six exported scripts match prior PCM/timing oracles, master remains unchanged and no generation is added. Client library tests pass 340 with nine ignored (the new native fixture separately passes); both strict lints, Windows source check, native build, formatting and architecture/source guards pass. Initial test compile/lint failures are retained. See [scope](PULSAR_DESKTOP_NEUTRAL_BUNDLE.md). Interactive GUI acceptance, native desktop conflict/resume, non-Linux runtime/durability, real-media/model/edition/vendor, Handy and human/release gates remain open. Full roadmap PARTIAL.


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


## 2026-10-16 — Handy qualification dossier and review checkpoint

[ARCH171](PULSAR_ARCHITECTURE_DECISIONS.md#arch171--retained-qualification-evidence-and-explicit-operator-review)
implements bounded evidence intake, separate operator review/withdrawal and fresh accepted-record
matching during read-only setup inspection. The [checkpoint](evidence/roadmap-2026-10-16/handy-qualification/checkpoint.json)
records focused software checks and isolated native recovery. No real acceptance or device
contact occurred. Physical-dynamics admission, operative settings, qualified setup, controller/clock
integration and actual hardware/model/media/platform/human/release gates remain open.
Full roadmap PARTIAL; no milestone is declared complete by this checkpoint.

The [sample-envelope follow-on](evidence/roadmap-2026-10-16/handy-sample-envelope/checkpoint.json)
now enforces accepted discrete limits on the verified quantized samples. This closes that
specific software check; physical trajectories, operative settings, setup and controller/clock
integration remain open.


## 2026-10-16 — operative settings and versioned qualification binding

[ARCH172](PULSAR_ARCHITECTURE_DECISIONS.md#arch172--explicit-operative-settings-in-versioned-qualification-subjects)
implements complete declared slider/stroke observations, exact version-2 subject matching
and legacy history preservation without inferred settings or approval. The
[checkpoint](evidence/roadmap-2026-10-16/handy-operative-settings/checkpoint.json) records
317/88/19 Protocol/Handy Engine/Handy Client passes, strict lint/build checks, both native
subject-version recoveries and preceding-reader refusal before database changes. This closes
the software observation/binding step; physical behavior, firmware compatibility, setup,
controller/clock and independent model/media/platform/human/release gates remain open.
Full roadmap PARTIAL; no actual device acceptance is created.


## 2026-10-16 — durable setup handoff backend

[ARCH173](PULSAR_ARCHITECTURE_DECISIONS.md#arch173--durable-setup-claims-and-read-only-journal-refusal)
implements durable setup claims, exact request/hosting binding, route reservation and restart
fencing, plus a reproduced/fixed unsupported-journal WAL checkpoint issue. The
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-journal/checkpoint.json) records final
8/51/88 setup/playback/Handy checks, strict lint/build and native migration/refusal evidence.
Public setup transport, request resolution, authority/route acquisition and loaded-program
reconciliation remain required integration work. Full roadmap and actual qualification remain
PARTIAL; no physical device action is performed by this step.


## 2026-10-16 — original setup lookup and keyed route guards

[ARCH174](PULSAR_ARCHITECTURE_DECISIONS.md#arch174--actorrequest-setup-recovery-and-private-route-namespaces)
connects historical actor/request recovery to the CLI and adds private route namespaces and
logical-device guards. The [checkpoint](evidence/roadmap-2026-10-16/handy-setup-recovery/checkpoint.json)
records scoped tests, native seeded-history recovery and preceding-reader refusal. These
complete the lookup and route-identity prerequisites, not public setup sending or physical
qualification. Setup transport, handoff authority, loaded-program/reconciliation and the
remaining controller/clock/model/media/platform/human/release gates stay open.


## 2026-10-16 — guarded public setup sender

[ARCH175](PULSAR_ARCHITECTURE_DECISIONS.md#arch175--explicit-qualified-setup-sender-and-cross-store-record-links)
connects accepted qualification, fresh preflight, keyed routes and the single-use journal
to the explicit setup CLI/Protocol command. Durable cross-store links prevent silent reset
after effect-history loss. The [checkpoint](evidence/roadmap-2026-10-16/handy-setup-send/checkpoint.json)
records focused tests, actual loopback PUTs and native refusal/replay/storage checks. No real
hardware action or qualification occurred. Loaded-program correlation, reconciliation,
mode/controller/clock and synchronized playback integration and independent acceptance gates
remain open; full roadmap PARTIAL.

## 2026-10-21 — exact unclaimed setup cancellation

[ARCH176](PULSAR_ARCHITECTURE_DECISIONS.md#arch176--exact-unclaimed-setup-cancellation-with-separate-recovery-receipts)
adds a public local cancellation with exact original-request/record pins and a separate durable
pending/settled receipt. The [checkpoint](evidence/roadmap-2026-10-16/handy-setup-cancel/checkpoint.json)
records focused tests, strict checks and actual CLI lost-reply/restart recovery. Claimed,
acknowledged and uncertain attempts remain reserved; cancellation supplies no stopping proof.
Next, retain the validated setup response and correlate a fresh stream observation before
constructing native loaded-program authority. Controller/server-clock integration, attempted
effect reconciliation, synchronized playback and actual independent acceptance gates remain
open. Full roadmap PARTIAL; goal active.

## 2026-10-21 — retained setup observations

[ARCH177](PULSAR_ARCHITECTURE_DECISIONS.md#arch177--atomic-retained-setup-observations-with-explicit-legacy-absence)
retains sanitized setup response fields atomically with acknowledgment in journal version 3.
Original intent identities survive migration; legacy missing response data stays absent and
new required observations cannot disappear silently. The
[checkpoint](evidence/roadmap-2026-10-21/handy-setup-observation/checkpoint.json) records focused
software checks, transaction-failure/corruption tests, native historical lookup/restart and
preceding-reader refusal. Database/WAL bytes are unchanged by that refusal; SQLite shared-memory
sidecars change. No current loaded-program authority follows from saved history.
Next, correlate fresh complete device state with the retained stream and exact hosted sample
shape, then connect current authority, controller ownership and measured server-clock mapping
to the existing playback runtime. Attempted-effect reconciliation, browser synchronization and
actual model/media/platform/device/human/release gates remain open; full roadmap PARTIAL.


## 2026-10-22 — fresh stopped-stream association

[ARCH178](PULSAR_ARCHITECTURE_DECISIONS.md#arch178--fresh-stopped-stream-association-with-streaming-buffer-semantics)
connects the original acknowledged setup and captured mode generation to explicit GET-only
CLI inspection with current authority and two complete HSP buffer samples. HSSP can stream a
script larger than the buffer, so expected script shape remains separate from the current
buffer. The [checkpoint](evidence/roadmap-2026-10-21/handy-program-correlation/checkpoint.json)
records scoped tests, native refusal/restart and preceding-reader checks. A saved report grants
no native admission or exact remote-content proof.
Next, construct native playback authority from independently rechecked current facts, retain
and retire that authority with controller/session lifetime, and acquire the host/server clock
mapping needed by the existing runtime. Attempted-effect reconciliation, actual browser/device
synchronization and model/media/platform/human/release gates remain open. Full roadmap PARTIAL;
goal active. No real setup, playback or physical acceptance occurred in this campaign.


## 2026-10-22 — native nonplaying playback preparation

[ARCH179](PULSAR_ARCHITECTURE_DECISIONS.md#arch179--boot-local-nonplaying-preparation-with-durable-recovery-and-retirement)
retains current verified program/timing/credential inputs with a disarmed runtime and transport
binding. Durable receipts precede native installation; missing state stays retired after
replay/restart. Existing mutation boundaries retire dependencies before replying. Matching
recorded play/stop effects preserve uncertainty through a complete existence query. The
[checkpoint](evidence/roadmap-2026-10-22/handy-playback-preparation/checkpoint.json) records scoped
tests, immediate retirement checks, actual lost retirement replies and native restart recovery.
No controller, arm, play or stop is created by preparation.
Next, add a distinct live-controller lifetime and native server-clock acquisition, keeping
server time, device HSTP synchronization and browser presentation mapping separate. Public
physical dispatch and stop/reconciliation integration, real-device/model/media/platform/human
and release gates remain open. Full roadmap PARTIAL; goal active.


## 2026-10-22 — renewable local controller lifetime

[ARCH180](PULSAR_ARCHITECTURE_DECISIONS.md#arch180--renewable-local-controller-leases-with-independent-expiry)
adds explicit local arming, generation-checked renewal, exact release and status. An owned
expiry worker revokes authority independently of API traffic and DB contention; no paired
session or saved receipt restores a missing lease. The
[checkpoint](evidence/roadmap-2026-10-22/handy-controller-leases/checkpoint.json) records scoped
software tests, clock/worker failures, actual idle and writer-contention expiry, native
lost-release replies and restart recovery. No physical play or stop is dispatched.
Next, acquire a native server-clock mapping, separately handle device HSTP synchronization
and browser presentation timing, and connect qualified physical dispatch/stop reconciliation.
Platform/suspend/device, model/media, human and release gates remain open. Full roadmap PARTIAL;
goal active.

## 2026-10-22 — native server-clock acquisition

[ARCH181](PULSAR_ARCHITECTURE_DECISIONS.md#arch181--explicit-server-clock-contract-and-native-observation-custody)
adds a credential-free server-time sampler, explicit version 3 origin/error contract and
native custody under the exact controller generation. Original retries cannot resample or
refresh age; acquisition rechecks expiry after receipt commit. The
[checkpoint](evidence/roadmap-2026-10-22/handy-server-clock/checkpoint.json) records scoped
arithmetic, HTTP, authority/race/restart tests, strict lint/build checks and an isolated CLI
historical-receipt campaign. No actual vendor/device acquisition or physical acceptance occurred.

Next, implement typed read-only device HSTP and user-offset observations with explicit
qualification semantics. The vendor observation lacks a last-sync timestamp or guaranteed
uncertainty, so do not infer fresh synchronization from its RTT. Combine server/device error
budgets explicitly. Then connect one owned runtime/dispatcher and completion/stop lifecycle,
consume retained controller stop intents, and bind browser source presentation timing before
claiming synchronized playback. Physical dispatch remains disabled. Model/media, platform,
device, human and release gates remain open. Full roadmap PARTIAL; goal active.

## 2026-10-22 — controller-scoped device timing observations

[ARCH182](PULSAR_ARCHITECTURE_DECISIONS.md#arch182--device-timing-observations-preserve-unknown-synchronization)
adds explicit read-only device uptime/offset/latest-sync RTT and user-offset inspection. Two
bracketed timing pairs retain changing values; device context and exact controller generation
are checked before and after I/O. Errors retire only the selected generation. A saved report
cannot replace a fresh query or restore a controller after restart. The
[checkpoint](evidence/roadmap-2026-10-22/handy-device-timing/checkpoint.json) records scoped wire,
HTTP, authority/race/restart, client and actual isolated CLI checks, plus strict lint/build gates.
No real device operation or acceptance occurred.

Next, implement explicitly authorized synchronization and a native device timing contract,
including user offset and combined server/device error accounting. The inspection API cannot
establish synchronization age or accuracy. Connect a single owned runtime/dispatcher with
durable completion and stop consumption, then bind browser source presentation and controller
heartbeat before claiming synchronized playback. Keep independent device, model/media,
platform/storage, human and release gates open. Full roadmap PARTIAL; goal active.

## 2026-10-22 — explicit synchronization custody and recovery

[ARCH183](PULSAR_ARCHITECTURE_DECISIONS.md#arch183--independent-synchronization-custody-and-single-use-effect-claims)
adds separate synchronization intents, witnesses, one-use claims, atomically bound observations,
original-request status and exact unclaimed cancellation. Controller/server-clock authority
retires before handoff. Unknown custody survives native retirement and blocks matching routes
or logical devices; cancellation cannot clear attempted effects. The
[checkpoint](evidence/roadmap-2026-10-22/handy-clock-sync/checkpoint.json) records scoped source,
HTTP/drop/deadline, journal/race/authority/restart and client checks, strict lint/build gates,
actual isolated CLI cancellation reply loss and preceding-reader refusal. No real device or
acceptance was used; transport acknowledgment remains an observation.

Next, retain and qualify native device timing facts under an explicit device error/age/user-offset
contract, compose their uncertainty with the native server mapping, and define reconciliation
for attempted synchronization. Then connect one owned runtime/dispatcher/completion/stop lifecycle
and browser presentation/controller heartbeat. A saved synchronization report cannot be promoted
to native timing authority. Real model/media/device/platform/storage/human/release gates remain
open. Full roadmap PARTIAL; goal active.


## 2026-10-22 — combined native device/server timing

[ARCH184](PULSAR_ARCHITECTURE_DECISIONS.md#arch184--native-synchronization-lifetime-and-combined-playback-clock-expiry)
adds an explicit version-4 device timing contract, bounded pending/completed native synchronization
contexts and combined-clock acquisition for a fresh controller. Pending contexts participate in
dependency retirement even after their original preparation disappears. Original synchronization
age survives controller renewal/replacement; replay and restart never recreate native facts.
The runtime now enforces an absolute source deadline in addition to command age and summed drift.
The [checkpoint](evidence/roadmap-2026-10-22/handy-playback-clock/checkpoint.json) records synthetic
positive-chain and refusal tests, strict lint/build gates and final-binary isolated CLI recovery.
No real device, synchronization, measured accuracy or physical acceptance was used.

Next, connect one owned runtime/dispatcher/completion/stop lifecycle, consuming only current
combined timing at the final handoff with a consistent BOOTTIME clock. Current controller logic
assumes local Armed state and no prior physical-effect history; extend it deliberately for live
playback and stopping uncertainty before enabling public play. Preserve stop delivery after
controller/grant loss and reconcile uncertain synchronization/play/stop outcomes. Browser source
presentation mapping and heartbeat, real media/model/device/platform/storage/human/release gates
remain open. Full roadmap PARTIAL; goal active.


## 2026-10-22 — dispatch clock and late-result custody

The [owned dispatcher](PULSAR_HANDY_DISPATCH.md) now uses the same suspend-aware clock
as native controller leases, preserving original source, command and lease expiry through
claim and HTTP handoff. It checks timing again before sending and after receiving; timing
loss after a single-use claim remains Unknown. Stop does not depend on a live play clock or
lease. A late attempted completion can retain exact historical metadata without changing
revoked runtime authority or consuming a newer stop; metadata failures retain that observation.
The [checkpoint](evidence/roadmap-2026-10-22/handy-dispatch-clock/checkpoint.json) records actual
local HTTP and SQLite failure/retry cases with injected clocks, 61 playback-runtime, 166 Handy
Engine and 19 journal tests, both strict lint configurations, builds and source/format guards.
No physical device, actual suspend, browser timing or acceptance evidence was created.

This repairs prerequisites discovered while mapping the owned execution integration; it does
not complete that integration. Next, connect one shared runtime, dispatcher, completion pump
and retained stop obligations to Engine preparation/controller custody, with final authority
serialized against dependency mutations. Preserve independent expiry under DB contention;
never let a worker retain the last Engine reference and join itself on teardown. Separate
active controller custody from the Armed/no-history predicates before exposing play. Complete
uncertain-effect reconciliation, browser presentation/heartbeat and actual device/model/media/
platform/storage/human/release qualification. Physical playback remains disabled; full roadmap
PARTIAL; goal active.


## 2026-10-22 — Engine-owned retained stops and claim ordering

[ARCH185](PULSAR_ARCHITECTURE_DECISIONS.md#arch185--independent-retained-stop-ownership-and-guarded-dispatch-claims)
connects retained controller-stop intents to an independent worker sharing the preparation's
runtime/transport. It captures current intent without I/O, records outside control ownership,
rechecks before one dispatch, observes completion and retries only metadata writes. Journal
writer contention returns promptly and restores the ordinary timeout. Repeated retirement does
not mint a fresh stop. Direct Engine destruction drains and joins owned workers without an
Engine lifetime cycle. The dispatcher authorizer now invokes an invocation-bound claim gate
inside its mutation guard; transport begins after that callback returns.

The [checkpoint](evidence/roadmap-2026-10-22/handy-stop-owner/checkpoint.json) records 66 playback,
170 Handy Engine and 20 journal tests, both strict lint configurations, App/Worker builds and
format/architecture guards. Real Engine/SQLite/local-HTTP scenarios cover idle expiry with the
project DB held, blocked journal writers, lost/acknowledged stop replies, failed metadata writes
without resend, and in-flight stop joining on Engine destruction. No real device, synchronization,
physical stopping proof or human acceptance was created.

Next, add the native play operation and durable original-request/witness/status lifecycle to
this owner. Extract fresh final authorization without retaining Engine, share the original DB
mutex, and hold it through the existing claim continuation to serialize against mutation plus
native retirement. Extend controller status from Armed/no-history admission to current Playing
custody without weakening fresh admission. Reuse the same owner for play and its resulting stop
rather than introducing a second dispatcher. Public recovery, graceful daemon listener/signal
shutdown, emergency stopping on storage failure, uncertain-effect reconciliation, browser source
presentation/heartbeat and real device/model/media/platform/storage/human/release gates remain
open. Full roadmap PARTIAL; goal active.


## 2026-10-22 — durable original play intents and claims

[ARCH186](PULSAR_ARCHITECTURE_DECISIONS.md#arch186--original-play-operation-identity-and-durable-claim-boundary)
adds journal v5 original play-operation records bound to the actor/request fingerprint, exact
acknowledged setup and derived preparation, controller, qualification, clock snapshot and effect.
Preparation commits the operation/effect together; only a durable single-use claim releases the
dispatch receipt. Completion updates both records atomically. Explicit unclaimed cancellation
never clears runtime uncertainty, and legacy effects cannot be adopted as known-unsent work.
The [checkpoint](evidence/roadmap-2026-10-22/handy-play-intents/checkpoint.json) records 32 journal,
66 playback and 170 Handy Engine tests, both strict lint configurations, builds and source/format
checks. Actual isolated binaries verify v4 refusal, v5 history retention and missing-effect refusal.
Database and existing WAL bytes are preserved; an absent WAL may be created empty by SQLite.
No device, real playback/synchronization or human acceptance was involved.

Next, reserve the original request through Engine authority before journal preparation, commit
a main-request witness before queue/claim, and wire original-history status and unclaimed
cancellation without recreating native work on replay. Extend the existing owner to play and
its independent stop/completion obligations; use the typed operation completion path for linked
plays, since generic effect completion now refuses to bypass it. Extract fresh play checks into
an independent context, share the original DB mutex, and retain it through the guarded claim.
Split active Playing custody from Armed/no-history admission. Public play, graceful daemon
shutdown, emergency stopping, uncertain-effect reconciliation, browser correspondence and
actual device/model/media/platform/storage/human/release gates remain open. Full roadmap PARTIAL;
goal active.


## 2026-10-22 — guarded native play submission

[ARCH187](PULSAR_ARCHITECTURE_DECISIONS.md#arch187--guarded-native-play-submission-and-shared-execution-ownership)
connects original play/status/unclaimed-cancel commands to the Engine's retained execution owner.
The original main request and exact journal witness precede queueing. Fresh final authority holds
the shared metadata/control guards through claim, and play/stop share a runtime and dispatcher.
Metadata recovery never requeues; unknown play completion cannot block its independent stop.
Playing controller renewal preserves original clock age. Journal v6 fences readers that lack
play-route exclusion. See the [submission guide](PULSAR_HANDY_PLAY_SUBMISSION.md) and
[source-bound evidence](evidence/roadmap-2026-10-22/handy-play-submission/checkpoint.json).

Next, reconcile attempted effects without treating transport acknowledgment as physical proof,
complete emergency stop behavior when persistence fails, and connect daemon signal/listener
shutdown to joined execution ownership. Then integrate browser presentation/heartbeat authority
and continuous timing renewal with the same owner. Real-device qualification still requires
actual credentials/firmware and measured motion/stopping/synchronization evidence. Real model,
media, platform, storage, human and release gates remain open. Full roadmap PARTIAL; goal active.


## 2026-10-22 — owned Engine daemon shutdown

[ARCH188](PULSAR_ARCHITECTURE_DECISIONS.md#arch188--owned-daemon-shutdown-and-joined-background-work)
connects signals, listeners and background task ownership to explicit Engine retirement. The
[checkpoint](evidence/roadmap-2026-10-22/engine-shutdown/checkpoint.json) records full default
Engine tests, the optional WASM filter, launcher checks, both strict lint configurations and
native Linux signal/IPC/startup-failure recovery. Composed localhost tests retire an active Handy
play, fence a pending claim, join stops and report worker panics while an external Engine reference
remains. Seventy default-suite and seven WASM-filter environment-dependent ignores remain explicit.

Next, implement attempted-effect reconciliation and emergency stop handling when the journal
cannot accept writes. Keep physical stop disposition distinct from transport observations.
Connect browser presentation/heartbeat and continuous timing renewal to the same owner only after
those recovery paths are concrete. Real-device shutdown, GPU/native storage failure, model,
real-media, platform, human and release qualification remain open. Full roadmap PARTIAL; goal active.


## 2026-10-22 — emergency stop fallback and incomplete status

[ARCH189](PULSAR_ARCHITECTURE_DECISIONS.md#arch189--emergency-stop-custody-without-a-confirmed-prewrite)
connects journal-unavailable stops to the shared native dispatcher without a fabricated receipt.
Durable and emergency paths share one runtime claim; play cannot use the fallback. Exact attempted
outcomes remain in memory until an atomic retrospective write succeeds. Journal v7 adds provenance
and corruption/missing-record checks. `handy-stop-status` exposes bounded original-session history
and retained activity, always with incomplete-history disclosure.

The [checkpoint](evidence/roadmap-2026-10-22/handy-emergency-stop/checkpoint.json) records the full
default Engine suite (1,138 pass, 70 explicit ignores), focused protocol/client/journal/runtime and
owner checks, both strict lint configurations, builds and native CLI/v6-refusal/corruption evidence.
The software path now attempts a stop despite held journal access or failed inserts; this does not
qualify real storage failure, disconnected-device stopping, or physical response.

The successor below implements the declared software reconciliation path. Continuous clock renewal,
browser presentation/heartbeat and streaming generated motion remain required, as do actual
model/media/platform/device/storage/human/release gates. Full roadmap PARTIAL; goal active.

## 2026-10-22 — qualified custody reconciliation and explicit resumption

[ARCH190](PULSAR_ARCHITECTURE_DECISIONS.md#arch190--exact-custody-resolution-with-fresh-qualified-observations)
adds exact unresolved-history plans and an owned asynchronous recovery worker. Only a current
accepted subject-v5 recovery contract can interpret fresh matching observations after targeted
native dispatch drains. Old physical identity/settings must match; route hashes alone cannot
resolve a different device. Journal v8 records separate immutable resolution membership and releases
only those routes. Original outcomes and old claims remain unchanged and cannot regain authority.
Cancellation receipts and original cancelled state are atomic. Replay repairs metadata only.

Unrelated renewals and timing reads remain available while selected preparations are retired;
command handles let drain/finish wait without holding the execution-owner mutex. A later explicit
setup, preparation, controller, timing and play sequence is required for resumption. See the
[guide](PULSAR_HANDY_RECONCILIATION.md) and
[checkpoint](evidence/roadmap-2026-10-22/handy-reconciliation/checkpoint.json).

Validation passes 1,172 default Engine tests (70 existing ignores), 353 Protocol and 52 Handy client
tests (one existing desktop ignore), both strict lint configurations, builds and structural/format
guards. Synthetic native CLI histories verify restart, metadata repair, preceding-v7 refusal and
six corruption cases; all 41 owned processes joined without forced kills. Existing DB/WAL bytes
are preserved, with absent-to-empty WAL creation recorded separately. No real device was contacted.

Next, bind actual browser presentation and independent player liveness to native controller/timing
ownership, and implement timing refresh during Playing. A connected precomputed-script path is an
integration step, not fulfillment of on-the-fly generation: continuously generated motion still
needs streaming-program/transport support. Arbitrary tab capture also lacks source-player identity,
pause/seek/rate observations. Preserve the complete roadmap and 200 ms capture-to-command target;
physical response and all actual qualification gates remain separate. Full roadmap PARTIAL.


## 2026-10-22 — independent timing refresh while Playing

[ARCH191](PULSAR_ARCHITECTURE_DECISIONS.md#arch191--independently-observed-timing-during-playing)
adds a separate native phase observation for an acknowledged existing play, with original
reference/error preservation, still-live handoff, exact generation/attempt fencing and independent
stop ownership. Schema6 qualification must physically substantiate the selected readback model.
The [feature guide](PULSAR_HANDY_CONTINUOUS_TIMING.md) records the arithmetic, renewal, replay,
retirement and CLI contract, plus successful software and isolated native refusal/compatibility
checks. Positive Engine sampling remains injected; localhost GETs are a separate test boundary.
No actual Handy timing, stopping or acceptance is claimed.

Next implement browser presentation-clock calibration and an independent player heartbeat bound
to exact native controller ownership, including pause/seek/end and stale page replacement. Then
carry newly generated motion through an HSP stream with bounded queued points, buffer-horizon and
starvation handling, and unknown-append recovery. A precomputed-script browser path alone cannot
fulfill on-the-fly generation. Preserve arbitrary-tab source/player correspondence, the 200ms
capture-to-command target, and all model/media/platform/physical/human/release gates. Full roadmap
PARTIAL; goal remains active.


## 2026-10-22 — independent browser player control and native stop guards

[ARCH192](PULSAR_ARCHITECTURE_DECISIONS.md#arch192--independent-browser-liveness-and-exact-native-stop-guards)
connects scoped browser liveness to optional native retirement of an existing acknowledged play.
HTTP/control remains responsive during stalled Preview work; exact page/player scope fences late
requests and results. Native liveness/release bypasses project DB access using an installed cached
principal, with public revocation retirement and independent BOOTTIME expiry. Lost/foreign release
responses retain original IDs and fail shutdown after workers join; no physical stopping is inferred.
See the [guide](PULSAR_BROWSER_PLAYER_CONTROL.md) and
[checkpoint](evidence/roadmap-2026-10-22/browser-player-control/checkpoint.json) for exact evidence.

The real private-Engine campaign validates HTTP lifecycle and authored inference; positive guard
semantics use synthetic/local fixtures. Rendered browser, actual Handy and model quality remain
unqualified. Next bind immutable source/placement and a justified presentation clock to native
synchronized start, add periodic controller/Handy timing maintenance, then carry generated motion
through HSP points with bounded horizon/starvation/unknown-append handling. Preserve arbitrary-tab
source-player correspondence, the 200 ms capture-to-command target and all M1-M8/Phase B-C gates.
Full roadmap PARTIAL; goal remains active.

## 2026-10-22 — imported browser media and declared placement clocks

[ARCH193](PULSAR_ARCHITECTURE_DECISIONS.md#arch193--verified-browser-source-reads-and-declared-clock-correspondence)
connects exact imported bytes, active placement and an explicit declared browser/source mapping
through prepared playback, native capture and retained motion provenance. Source verification,
bounded Range delivery and player/inference control have independent ownership. A native pin
cannot be recovered from history; pause/seek retire playback while preserving selected read access.

The [guide](PULSAR_BROWSER_SOURCE_MEDIA.md) and
[checkpoint](evidence/roadmap-2026-10-22/browser-source-media/checkpoint.json) record software,
actual native HTTP, rendered authored-video inference, handoff and signal-cleanup evidence.
A reproduced status-readiness race and queued-pause defect are fixed; neither diagnostics nor
green tests substitute for actual source/presentation or physical qualification.

Next establish calibrated browser/native presentation correspondence and bind it to exact
source/script provenance for synchronized start. Add periodic controller/device timing
maintenance and generated HSP points with bounded horizon/starvation/unknown-append handling.
Keep arbitrary-tab correspondence, 200 ms capture-to-command, useful model/media accuracy,
six-axis, platform, hardware, human and release obligations. Full roadmap PARTIAL; goal active.

## 2026-10-22 — browser callback timing before synchronized start

[ARCH194](PULSAR_ARCHITECTURE_DECISIONS.md#arch194--source-bound-browser-execution-brackets-before-play)
connects native probe ownership to a fresh browser callback, exact prepared source and declared
project phase without requiring an already-Playing Handy controller. Original history remains
inert; expiry, public source retirement and pause/replacement/shutdown clear the exact lifetime.
The [guide](PULSAR_BROWSER_TIMING.md) and
[checkpoint](evidence/roadmap-2026-10-22/browser-timing/checkpoint.json) distinguish conditional
interval math, authored native HTTP observations and actual rendered-browser evidence.

The rendered campaign observes 230 timing exchanges, two capture handoffs and 683 accepted
diagnostic estimates. Inactive pause history does not rearm; active SIGTERM retires capture,
media and timing while Engine remains alive. The source mapping is still declared, and
callback execution is not physical display time. No actual Handy was contacted.

Next define and consume accepted browser/source/display bounds and exact script correspondence
in a native start plan. Its phase and server reference must share one native anchor, with
player/source ownership continuous through queued and acknowledged play. Then connect generated
points to a bounded HSP stream owner with starvation and ambiguous-append handling. Keep the
full model/media, arbitrary-tab, 200 ms, six-axis, platform, device, human and release scope.
Full roadmap PARTIAL; goal active.

## 2026-10-23 — browser-observed prepared Handy Start

**PROGRESS; full roadmap PARTIAL and goal active.** The
[browser Start contract](PULSAR_BROWSER_HANDY_START.md) connects exact source/script
correspondence and accepted browser/source measurement bounds to existing HSSP
submission and stop custody. The original phase and server-time reference share
one native anchor; queued dispatch, ongoing observations and owner renewal cannot
refresh that original decision. Pre-arrival cancellation fences the original
request. Journal v9 retains exact membership without rewriting legacy records.
The browser has explicit inspection, Start, original history and Stop controls,
with separate native Start/history, renewal/release and device-refresh workers.

Final default checks pass 2,538 cases: Core199, Protocol404, Engine1,275
(71 ignored), Clients558 (14 ignored) and JavaScript102. Both strict lint
configurations, builds, architecture, source guards and formatting pass. Actual
v8/v9 executables pass empty and populated journal compatibility and preservation
checks. The rendered browser passes diagnostic playback, missing-prerequisite
Start refusal and active SIGTERM cleanup with Engine alive during retirement.
Its earlier cancellation-only timing failure has a failing IPC regression and
passing repaired rerun. Timing/contention fixture corrections preserve production
deadlines; earlier failures and interrupted/timeout runs remain in the
[checkpoint](evidence/roadmap-2026-10-22/browser-start/checkpoint.json).

The populated journal uses authored Physical/LocalHumanAcceptance/Acknowledged
labels. Positive playback tests use localhost HTTP and injected observations.
Neither establishes real physical acceptance, and the rendered campaign does not
exercise successful Handy Start. No real device was contacted. Controller/clock
expiry still requires fresh preparation and a new configured launch. Next work is
simpler preparation/reacquisition and a bounded generated HSP stream owner with
starvation, cancellation and ambiguous-append handling. Useful motion, real-media
and browser/source measurements, arbitrary-tab correspondence, the 200 ms target,
six-axis/platform/device/human/release acceptance remain required.


## 2026-10-23 — generated HSP stream components

The [bounded stream queue, codec and coordinator](PULSAR_HANDY_GENERATED_STREAMS.md)
now have 45 passing new cases within Core215 and Engine1,304 (71 existing ignores),
plus strict lint and structural checks. They retain exact append custody, source
coverage, original timing and explicit Play/Stop. The source-backed pre-Play option
correction avoids assuming Setup establishes options that Play later writes.

Next compose durable operation/claim history and shared physical-device ownership,
then accepted stream qualification, native dispatch and the checked inference
producer. Browser controls and presentation buffering follow that native path;
latest-estimate polling and receipt timestamps cannot replace delivery or source
clock evidence. No live toy streaming, physical behavior or model quality is
qualified by component tests. Full M1-M8 roadmap PARTIAL; goal active.


## 2026-10-23 — durable HSP custody and native sender

[Durable HSP dispatch](PULSAR_HANDY_HSP_DISPATCH.md) now binds original actor, source,
registration/device, credential route, timing and exact wire bytes to a single-use
journal-v10 claim. Legacy driver admission/reconciliation observes HSP custody.
Bounded native preflight and worker-bound handoff preserve original deadlines,
joined transport ownership and metadata-only retries. Definite unclaimed cancellation
can release the exact runtime slot for Stop; a rejected clone cannot clear a claim.

Full Core215/Engine1,354 (72 ignores), both strict configurations, builds and
structural checks pass. Actual preceding-v9/current-v10 readers pass populated
history/WAL preservation and restart checks. Initial invalid legacy fixtures, strict
style failures, RAM-capacity harness failure and ordinary startup timeout remain in
the evidence; the unchanged startup test passes later ordinary/RAM repeats without
a claimed product repair. Native HSP HTTP cases use localhost; history is synthetic.

Next compose public qualification/admission, continuous native ownership and emergency
stopping, HSP recovery and mode acquisition, then exact accepted-inference delivery
and browser controls/presentation. Useful learned live motion needs its own target/view
contract and actual annotated evidence; diagnostic graphs are not quality proof.
Keep every M1-M8 physical/model/media/platform/human/release obligation. Full roadmap
PARTIAL; goal active.

## 2026-10-23 — native HSP Stop during journal failure

[HSP emergency Stop](PULSAR_HANDY_HSP_EMERGENCY_STOP.md) adds a shared native
reservation for durable and emergency dispatch. A sealed already-issued Stop
can use retained native ownership when journal access is unavailable, preserving
single-use custody through cancellation, joined shutdown and uncertain replies.
An observed replacement still refuses. Its retrospective observation is distinct
from durable history and cannot resend.

This prerequisite does not complete continuous ownership. Retrospective journal
records, ordinary-slot retirement during metadata outage, post-Setup acquisition,
authenticated admission, accepted inference and browser controls remain. The
checkpoint linked from the implementation guide records actual checks; localhost
evidence does not close physical, model, media, platform, human or release gates.

## 2026-10-23 — Engine-owned HSP sessions

[The Engine-owned HSP lifecycle](PULSAR_HANDY_HSP_OWNER.md) now connects Setup,
post-Setup acquisition, prefill, explicit Play, appends, native-unsent retirement
and Stop with independent watchdogs and joined shutdown. Authenticated read-only
status reports retained original sessions without starting or renewing one.
Evaluation clocks are sampled under the runtime mutation guard; original sample
and Play timestamps remain intact across concurrent producer activity.

Positive lifecycle evidence uses localhost devices and authored facts. The linked
checkpoint records passing checks, failed attempts and exact source scope.
Next compose HSP-specific qualification/reconciliation and public admission, the
accepted-inference clock bridge, browser controls/presentation and persisted
emergency history. Retain useful-model, real-media, Handy 2 Pro, platform, human
and release obligations across the full M1-M8 roadmap. Full roadmap PARTIAL.


## 2026-10-23 — HSP qualification, emergency history and recovery

[HSP recovery](PULSAR_HANDY_HSP_RECOVERY.md) extends current evidence review with an
explicit generated-stream subject, retains separate emergency history in journal
v11 and resolves exact HSP custody after native drain and fresh qualified samples.
Owned metadata workers preserve independent watchdog/Stop progress during storage
work. Current native proofs and committed receipts release only matching targets;
replay restores no stream authority. The checkpoint preserves exact tests and
native reader/migration evidence, including failures and their scope.

Next implement genuine native HSP preparation/clock acquisition, original-request
admission, accepted-inference delivery with the capture/native clock bridge and
browser controls/presentation. Keep useful-model, actual-media, Handy 2 Pro,
platform, six-axis, human and release gates across M1-M8. Full roadmap PARTIAL.


## 2026-10-23 — native accepted capture timing

[Native capture correspondence](PULSAR_LIVE_CAPTURE_CLOCK.md) preserves original
relative ticks as suspend-aware intervals, detects discontinuity without rebasing,
and retires failed epochs with a valid cleanup tick. Accepted CPU results retain
both original frames and pairing continuity; stale and provisional CUDA output
cannot supply a usable native stamp. Ordinary preview remains available when no
native clock could be acquired initially.

The focused live suite, both strict lint configurations and full Engine semantic
suite on controlled RAM pass; exact process checks remain in the checkpoint. This
is a required producer timing seam, not public HSP admission or physical acceptance.
Next compose original native HSP preparation/synchronization custody, consumable
clock readiness, public controls, checked motion envelopes and measured browser
analysis lookahead. Full M1-M8 model/media/device/platform/human/release scope remains
PARTIAL, including the retained ordinary-host timing failures.


### 2026-10-23: original HSP preparation and native clock handoff

[ARCH202](PULSAR_ARCHITECTURE_DECISIONS.md#arch202--original-hsp-preparation-and-native-clock-handoff)
and [ADR B5-060](../research/phase-b/ADR-B5-060-hsp-preparation.md) connect original
request reservation, one-shot native synchronization and retained clock readiness
through public Prepare/Status/Cancel. Fresh clocks, current accepted qualification
and the original budget remain prerequisites; replay retries metadata only. A
private source handoff verifies the exact clock and uncertainty before and after
claim I/O. Journal v12 transfers custody only with the exact child's first Setup
claim and includes preparations in aggregate qualified recovery.

The [guide](PULSAR_HANDY_HSP_PREPARATION.md) and its checkpoint retain 23 focused
preparation passes, 1,462 full Engine semantic passes on controlled RAM, strict
lint/build/structural checks and actual v11/v12 reader/migration/restart results.
Ignored tests, corrected fixture failures, busy-journal metadata retry, prior ordinary
host timing failures and all physical limitations remain explicit.

Next deliver one connected production browser-source/CPU stream: native preparation
factory context, current source/presentation authority, direct accepted-result queue,
checked discrete motion envelope and original public admission/Play/Stop. Analyze
ahead of a separately observed visible player; a declared buffer does not create
actual presentation lead. Do not substitute test source facts or sampled status for
production authority. Useful model/media, long-running freshness, actual Handy2Pro,
platform/six-axis, human and release scope remain required. Full roadmap PARTIAL.


## 2026-10-23 — connected browser analysis and original HSP control

[ARCH203](PULSAR_ARCHITECTURE_DECISIONS.md#arch203--native-browser-analysis-feed-and-explicit-live-hsp-control)
connects the production CPU completion path, native source/presentation pins,
checked discrete motion and original Bind/Play/Stop. Source qualification and
current-environment acknowledgment remain separate from online control and Play.
The [guide](PULSAR_BROWSER_HSP_LIVE.md) records the exact public workflow and limits.

The actual CPU-worker/localhost HSP test verifies three accepted decoded-frame
results, Setup/Add/Play/Add/Stop, unchanged replay and startup integrity refusals.
Full Engine semantics pass 1,485 tests (74 ignored) on controlled RAM. Core 225,
Protocol 433, Clients 580 (14 ignored), browser JS 119, strict checks and builds pass.
Actual rendered preview exposed and closed future-estimate overwrite; preserved
original deadlines now permit current display with adequate lookahead margin.
The browser and HSP campaigns are separate; physical effects remain untested.

Exact final-input sealing/draining is integrated under ARCH204. Rendered campaigns
complete the full 30-second video and a 12-second trim; status-contention retirement
and upper-bound uncertainty refinement regressions are corrected. The sealed
[finite-input checkpoint](evidence/roadmap-2026-10-23/hsp-finite-tail/checkpoint.json)
retains those subjects and the earlier intermittent preclaim failures.

ARCH205 now retains the exact unclaimed command under bounded dependency contention,
releasing locks before revalidation and preserving original deadlines. Full Engine
1,534 tests (81 ignored), five actual CPU/localhost HSP cases in three unchanged-binary
runs, strict lint/format/structure checks and builds pass. Positive release sends
once; cancellation or the original cap sends no pending Append. Native tests retain
Pending/Synthetic qualification and prove no physical effects. Earlier ordinary-host
storage/timing failures remain unresolved by controlled-RAM checks. See the
[current preclaim checkpoint](evidence/roadmap-2026-10-23/hsp-preclaim/checkpoint.json).

Next implement sustained source/media transfers, separately owned fresh clock
observations and the integrated HSP user flow using the
[verified interface map](evidence/roadmap-2026-10-23/hsp-preclaim/sustained-next/README.md).
Do not extend original samples, sync age, pending command windows or Play references
to simulate continuity. [Primary-source timing/drain research](../research/phase-b/handy-hsp-sustained-timing-and-drain-2026-10-23.md)
retains the unknown qualified assumptions. Useful models, actual media, GPU/six-axis,
Handy2Pro, platform, human and release obligations remain binding across M1-M8.
Full roadmap PARTIAL; goal active.
