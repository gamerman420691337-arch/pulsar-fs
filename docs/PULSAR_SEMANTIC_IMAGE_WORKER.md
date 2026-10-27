# Bound semantic image assistant worker

Status: **worker foundation implemented and tested; roadmap PARTIAL; unqualified**.

## Implemented behavior

The trusted `Engine::propose_assistant` embedding API now accepts an optional vision request binding: exact projector identity, exact row-major RGB8 identity, width and height. Dimensions are 1–512 on each axis; byte length must equal width × height × 3. Vision inference is restricted to exclusively creative synthetic proposal scope. Existing text request/response JSON omits the optional field and retains the text completion CLI/EOG contract.

The parent requires exactly matching trusted asset paths, opens and validates their metadata, accounts both parent sealed copies and child mounts in the aggregate RAM/CPU admission, captures/hashes immutable memfds, and retains both under existing worker-fence custody. The fixed guest mounts are `/assistant/mmproj.gguf` and `/assistant/image.rgb`. The response must echo the original identities and dimensions; absence, changed IDs/digests/lengths or swapped bindings refuse.

Only the confined child converts bounded pixels into a private P6 PNM file. The native vision runtime uses its dedicated CPU-only flags and schema sampler. Strict whole-body JSON parsing permits whitespace but rejects diagnostics, extra or duplicate members, partial replies and unsupported proposals. The proposal grants no session, project, candidate, generation, approval or device authority.

## Current-source verification

| Check | Result and scope |
| --- | --- |
| Protocol | 126 PASS, including dimensions/exact RGB, exclusive creative scope and absent-field text compatibility |
| Default Engine | 835 PASS, 66 ignored; RAM-backed, network-isolated harness |
| All-feature Engine | 849 PASS, 92 ignored; same declared harness |
| Native parent tests | Five PASS in 115.66 seconds: real vision, real existing text assistant, existing transport/cancellation checks, and new vision faults |
| New vision fault fixture | Seven scripted cases: valid pair, changed projector, changed RGB, omitted binding, output excess, deadline and observed-live cancellation; empty custody and zero retained CPU after each |
| Workspace default build | PASS; new default application/worker identities retained |
| Strict workspace/all-target Clippy | Default and all features PASS |
| Structure/source guards | 402 and 36 PASS |
| Format and whitespace | PASS |

The native vision calls use the pinned runtime/model/projector from the preceding [runtime experiment](PULSAR_SEMANTIC_IMAGE_RUNTIME.md). The measured peak child cgroup RAM is 769,089,536 bytes for circle and 769,081,344 bytes for square, below the declared two-GiB child ceiling. Responses bind both exact pixel identities; the ledger is empty after both calls. The existing Qwen text target-selection test also passes with the new default worker.

The first direct filesystem-scratch Engine suite reached its 240-second timeout, without a complete result. The same executable subsequently passed all 835 tests under the previously used RAM-backed, network-isolated harness; final test fixtures then passed the final suite. The cause of the filesystem run's delay was not fully diagnosed, and persistent-disk performance remains unqualified. Three test-code compilation failures and their corrections are retained in the checkpoint; no compiler suggestion to widen production access or add a new default configuration was adopted.

## Model instruction quality: FAIL

The authored instruction asks for sine on a circle and triangle on a square. The real candidate emits a pulse (phase 0.5) for circle and `no_action` for square. Both are structurally valid, safely bounded proposals, but neither satisfies the requested mapping. This is a failure of these two task controls, not a useful-model acceptance result. The prior simple visual shape/color classification controls remain their separately scoped evidence.

## Remaining product work

Existing image generation still records `image_not_semantically_conditioning_motion` and synthesizes from the constrained prompt. The subsequent [authenticated prepared-image increment](PULSAR_AUTHENTICATED_IMAGE_ASSISTANT.md) implements trusted vision profile registration, supervised still normalization, pathless source/preprocessing references, authenticated start/report context and fresh approval checks. Owner runtime policy setup, CLI/desktop presentation and dedicated image-conditioned generation lineage remain open. The experimental upstream CLI and this candidate require independent runtime, installation and model-quality evaluation before defaults or distribution.

No annotations or training were fabricated. No user reference files, secrets, device state or physical hardware were used. Still imagery cannot establish observed temporal motion or faithful six-axis reconstruction. Real-media, GUI/platform, hardware, human and release gates remain open.

Decision: [ADR B3-130](../research/phase-b/ADR-B3-130-bound-image-assistant-worker.md). Evidence and exact subjects: [checkpoint](evidence/roadmap-2026-10-11/semantic-image-worker/checkpoint.json).
