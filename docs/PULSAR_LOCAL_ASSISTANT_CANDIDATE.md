# Local assistant candidate: real inference and proposal validation

Date: 2026-10-10. Status: **PARTIAL software foundation, FAIL candidate evaluation, full roadmap INCOMPLETE**.

Pulsar still has no application-integrated bundled assistant. This checkpoint runs a real learned model and implements a strict proposal decoder. It does not relabel the deterministic text preset parser as natural-language inference, select a release model, enable the disabled desktop control or close EDIT-01/TEST-23.

## Candidate and execution

The official Qwen2.5-0.5B-Instruct Q4_K_M artifact was downloaded without authentication and locally verified: 491,400,032 bytes, SHA-256 `74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db`, repository revision `9217f5db79a29953eb74d5343926648285ec7e67`. Download elapsed 10.213 seconds. First-party sources, licenses and alternatives are recorded in [runtime research](../research/phase-b/PULSAR_LOCAL_ASSISTANT_RUNTIME_RESEARCH_2026-10-10.md). That research note describes its researcher's read-only work; this subsequent checkpoint built and loaded the actual candidate.

An unchanged, clean llama.cpp checkout at `b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c` built `llama-completion` successfully in owned scratch. CUDA/Vulkan/Metal/BLAS, native tuning and AVX variants were disabled; SSE4.2/BMI2 remained enabled by upstream defaults. This is a current-host Linux candidate, not a generic portable CPU binary. llama/ggml are statically linked; system libgomp, libstdc++, libm, libgcc_s and libc remain dynamic dependencies. HTTPS support was disabled. Neither that build nor `--offline` constitutes a filesystem/network sandbox.

The standard-library evaluation harness runs noninteractive, single-turn, CPU-only inference with an exact runtime/model hash, 2,048 context tokens, 256 maximum generated tokens, four CPU threads, fixed seed/greedy sampling, 2-GiB address-space ceiling, 45-second deadline and separate 64-KiB stdout/stderr bounds. The child receives a small explicit environment without provider keys or LLAMA_ARG overrides. Timeout/output exhaustion kills and joins the owned process group. Every request reloads the model; these are not warm resident-model latency figures. Peak RSS, application memory and confinement were not measured or qualified.

The pinned runtime emits an exact ` [end of text]` trailer followed by three newlines. The experimental decoder requires that whole envelope and parses the entire remaining JSON body. It refuses missing EOG, multiple objects, preceding logs, duplicate members and unsupported fields. This version-specific envelope is not the future application IPC ABI. The Python harness is development/evaluation tooling; it is not a Python dependency for installed Pulsar.

## Observed results

Six fixed synthetic requests cover target selection, absent target, review-record selection, sample-stride settings, selected-range edit recipe and unsupported device effects. The same broad grammar permits every action and arbitrary reference strings; it does not force expected answers. Supplied context is synthetic, not real-media evidence.

| Request | Context inside user message | Separate system context |
| --- | --- | --- |
| Track second cup | PASS: cup2 | PASS: cup2 |
| Track absent elephant | FAIL: cup1 | FAIL: invented ID 1 |
| Explain flagged interval | FAIL: invented occluded_tracking ID | FAIL: same invented ID |
| Sample every fourth frame | FAIL: selects beaker1 | FAIL: proposes edit recipe |
| Smooth selected range | FAIL: recipe ID used as target | PASS: smooth_selection recipe |
| Start device movement | FAIL: selects beaker1 | FAIL: proposes edit recipe |

Every inference exited zero and produced parseable proposal syntax. Semantic accuracy was **1/6** and **2/6**; both campaigns exit 1. Request elapsed ranges were 8.27–10.08 and 7.86–9.58 seconds respectively. These tiny development probes are diagnostic, not statistical quality qualification. The candidate is not selected as the bundled default. A known valid identifier can still be semantically wrong: context validation does not replace application approval or model evaluation.

## Implemented proposal contract

`pulsar-protocol::AssistantProposal::decode` accepts one bounded strict JSON body and resolves target/review/edit references only against supplied bounded, unique host-issued IDs. Sample stride must be explicitly offered. NoAction has an empty struct representation so unknown members are refused. An initial unit variant admitted an unknown field; its red regression (111 passed, one failed) led to that correction.

The model cannot supply coordinates, arbitrary settings, edit values, protection replacement, evidence/confidence claims, paths, executable names, capabilities or device effects. ExplainReview selects an existing record; the future broker must display its actual recorded reason/confidence. ApplyEditRecipe selects a precomputed host entry; the future broker must apply it through the existing protected edit/history transaction. The decoder does not establish observation truth, session/project/revision freshness, authority, approval, cancellation or edit safety.

This adds exported data types only. No Command/Response variant, framing version, database schema, dependencies, engine dispatch or device authority changes. Target/review/settings/edit actions remain unimplemented at the assistant application boundary. See [ADR B3-082](../research/phase-b/ADR-B3-082-local-assistant-proposal-foundation.md).

## Verification and retained artifacts

Protocol library **112 PASS**, zero ignored; protocol contracts **10 PASS**; strict protocol all-target Clippy PASS; workspace formatting PASS; architecture **353 PASS** and source guards **36 PASS**. The first contracts build timed out at its 20-second deadline before test execution; the subsequent bounded run completed. Frozen-wire results are recorded separately in the packet. Full engine/client/workspace runtime suites were not rerun for these additive types; preceding results are historical subjects, not current full-workspace qualification.

Direct supervisor checks exercised real sleeping and oversize-output children, kill/join and exact-envelope refusal. Those checks and the initial protocol red result have observed summaries only; their complete original logs/binaries were not retained. Current successful logs, exact native configuration/build logs, both campaigns, publisher/local identities and source snapshots are retained in [evidence packet](evidence/roadmap-2026-10-10/local-assistant-candidate/checkpoint.json).

Retained candidate assets: `/home/brianklam/Projects/Funscripts/assets/pulsar-local-assistant-candidate-2026-10-10/` contains model, native executable, publisher identity, two license copies and manifest. These are experimental assets outside Git, not a complete redistribution bundle; dependency/license/platform audit remains open. Owned intermediate build/download scratch is removed after terminal jobs and retained-byte verification. Shared caches, external checkout, other models/services and prior scratch remain untouched.

## Remaining delivery

Continue actual learned-model evaluation, then implement engine-owned immutable runtime/model custody, aggregate admission, confined cancellable worker supervision, request/result identities, fresh-context approval and durable effect receipts. Wire first-party controls to real target/settings/review and protected undoable edits. Resource-aware generation pause/assistant/resume, bounded memory, offline installer/first launch, prompt injection/adversarial quality, supported platforms and rendered/human acceptance remain separate open gates. No annotations exist; sterile references remain evaluation-only and no useful training-quality claim follows from these synthetic requests. Handy 2 Pro qualification still needs the encrypted-secret setup and physical evidence.
