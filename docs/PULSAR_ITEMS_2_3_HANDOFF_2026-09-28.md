# Pulsar items 2-3: implementation handoff

## Scope and status

Resume work in `/home/brianklam/Desktop/Funscripts/pulsar-roadmap-m1`.

User objective: complete item 2 (production benchmark runner and evidence intake) and item 3 (explicit model/runtime/inference profiles). Neither is complete. Do not expand this task to items 4-18 or declare the Good phase complete.

This handoff summarizes retained conversation context. Source, repository state, and test results were NOT refreshed when this document was written. Treat paths and results below as navigation and historical evidence, not current verification.

## Working constraints

- Preserve all existing changes, including unrelated and earlier item-1 work. Work from the existing checkout rather than assuming its uncommitted changes exist elsewhere.
- No commit, push, destructive cleanup, service termination, or physical device actuation is authorized by this handoff.
- The user authorized builds, tests, integration checks, and repairs during implementation.
- Mandatory model reviews are suspended by the user's current AGENTS instructions. Independent release gates and human authorization remain distinct.
- This session was blocked by a developer-level no-reread rule. Follow the receiving session's actual instructions; this document grants no exception to them. Do not assume a new session removes that rule.
- No promise of flawless software or qualification from synthetic tests.

## Authoritative scope

Consult `docs/PULSAR_GOOD_PHASE_CLOSURE_MATRIX.md`, especially GB-01 and GB-02, and the existing architecture/specification records when permitted.

GB-01: production worker execution with exact immutable dependencies, retained raw outputs/errors/timing, independent oracle evaluation, rights and split gates, and honestly sourced human-effort evidence.

GB-02: explicit model bytes and preprocessing, reported executed provider and fallback, strict backend pins, and profile-bound quality/speed/memory evidence. CPU-only scaffolding or successful model loading does not satisfy the full requirement.

## Existing implementation map

### Benchmark intake and campaign

- `scripts/benchmark-intake.mjs`: bounded source/reference snapshots, rights/split checks, hashes, private staging, cleanup scoped to the failed invocation.
- `scripts/benchmark-campaign.mjs`: trusted runner identity, production attempts, retained receipts and errors, independent scoring, optional bound human-effort claims.
- `scripts/benchmark-funscript.mjs`: independent monotone-run conversion with plateau/reversal preservation.
- `scripts/benchmark-effort.mjs`: evidence identity and timing validation, not human authentication.
- `scripts/benchmark-onnx-fixture.mjs`: tiny authored input-dependent ONNX graph; not a trained detector.
- Corresponding `.test.mjs` files cover local and synthetic integration scenarios.

### Rust boundaries

- `crates/pulsar-protocol/src/inference_profile.rs`: declared profile, preprocessing/provider identities, resource admission, execution evidence, structural qualified-fallback transitions.
- `crates/pulsar-protocol/src/worker.rs`: optional `GenerationSettings.inference_profile`; omitted legacy default remains compatible.
- `crates/pulsar-engine/src/inference_profiles.rs`: shared strict profile admission and identity. Current supported execution is CPU RGB/NCHW stretch with the existing YOLO contract; unsupported profiles reject instead of silently falling back.
- `crates/pulsar-engine/src/benchmark.rs`: pinned worker execution, immutable request receipt, neutral-motion conversion and native review-flag mapping.
- `crates/pulsar-engine/src/authority.rs`: resolved profile identity added to generation dependencies.
- `crates/pulsar-engine/src/launch.rs` and `cgroup.rs`: production execution metrics and whole-worker cgroup peak memory.
- `crates/pulsar-engine/src/worker/detector.rs`, `telemetry.rs`, and `mod.rs`: explicit CPU registration and successful inference-run timing in the native lineage receipt. No per-node provider trace or inference-only memory measurement.
- Engine examples: `benchmark-attempt`, `benchmark-smoke`, `benchmark-motion`, `benchmark-generate`.
- Engine tests: `benchmark_motion`, `benchmark_review`, `benchmark_profiles`; protocol tests: `inference_profiles`, `inference_fallback`.
- Research: `research/phase-b/ADR-B1-002-benchmark-input-staging.md` and `ADR-B1-003-production-campaign-contract.md`.

## Remaining implementation and assurance

1. Finish actual CLI/GUI profile selection through shared engine contracts.
2. Implement engine-authoritative qualified fallback: qualification resolution, fresh attempts, bounded scheduling, persistence, strict pins, cancellation/revocation behavior. Existing protocol transition types alone are insufficient.
3. Finish model/runtime/profile catalog integration and accurately declare supported coverage.
4. Expand corruption, timeout, cancellation, interruption, recovery and dependency-drift campaigns.
5. Reconcile implementation and documentation against every GB-01/GB-02 acceptance condition.
6. Separate implementation completion from outstanding real-media, trained-model, hardware/performance and authentic human-effort qualification.

Audit candidates, NOT confirmed findings: whole-buffer runner hashing and path identity race; process-tree cleanup after campaign timeout; human-effort failure after scoring; top-level versus nested selected-profile behavior; canonical hash formatting; telemetry lifetime if requests ever share a process. Inspect or reproduce before changing these.

## Recorded evidence, not rerun at handoff

- Earlier engine library run: 203 passed, 1 ignored. This predates later changes.
- Profile integration tests after selected-profile wiring: 4 passed.
- Latest recorded sequence: client library tests passed; `pulsar` and `benchmark-generate` rebuilt; campaign test passed with the rebuilt binaries and `/usr/lib/libonnxruntime.so`.
- Campaign covered authored stationary Y4M, unsupported preset, short worker deadline, synthetic human-effort binding, and authored ONNX execution with CPU telemetry and worker memory.
- Retained root: `/tmp/pulsar-campaign-evidence-ns9gOz/`. It may no longer exist.
- These runs do not establish real-detector accuracy, realtime performance, GPU support or release qualification.

## Suggested restart prompt

Resume Pulsar items 2-3 from the existing checkout. Read this handoff and applicable guidance, then inspect current source and changes as your active instructions permit. Preserve existing work. Finish benchmark/evidence intake and explicit inference profiles against GB-01/GB-02, run the authorized relevant checks, repair demonstrated failures, and document remaining qualification gaps honestly. Do not commit, push or actuate hardware. If session instructions prevent necessary inspection, report that before making changes.
