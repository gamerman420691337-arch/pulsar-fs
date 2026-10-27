# ADR B3-130: Bound image conditioning in the assistant worker

Status: worker foundation implemented; production client/profile/approval integration and qualification remain open.

## Problem

Still-image generation currently binds image bytes to lineage but generates patterns from the prompt alone. The text assistant has no paired vision projector or pixel payload. Reusing its completion CLI flags and EOG envelope fails against the pinned multimodal CLI.

## Decision

Extend the private one-shot assistant request/response with an optional typed vision binding: paired projector identity, raw row-major RGB8 identity, width and height. Bound dimensions to 1–512 on each axis and require exactly width × height × 3 bytes. Only exclusively creative synthetic scope may request image inference. Absence preserves existing text JSON envelopes and invocation.

The trusted Engine embedding caller supplies exactly two additional paths, never an RPC client. Before capture, open regular files with exact declared lengths, charge sealed parent bytes plus all copied child mounts to the existing aggregate RAM/CPU reservation, capture/hash immutable memfds, and retain both images under worker-fence custody. Bind the response to all original identities and dimensions. Missing, extra or changed assets refuse; no ambient model or media discovery is introduced.

Inside the confined child, verify both mounted identities under the whole-worker deadline. Encode already bounded raw pixels as a fixed P6 PNM file in private scratch; native parsing remains in the child. Use a dedicated CPU-only MTMD invocation and strict whole-body proposal decoding. Keep the existing finite creative grammar and independent proposal validator. Child failure, output excess, timeout or cancellation yields no effect; existing process-tree retirement retains admission until observed.

This is the worker foundation for the final image-conditioned product flow. The existing authenticated text profile/start/report/approval surfaces do not yet advertise vision capability. Their registration must refuse additional vision mounts until a corresponding profile contract exists. The trusted embedding API returns only an untrusted proposal and grants no session, revision, candidate, project, approval or device authority.

Next: explicit owner vision profile/runtime policy and exact source/preprocessing bindings; child-only still-image normalization; authenticated start/report context and fresh creative approval checks; CLI/desktop presentation and separate reviewed generation; retained image-conditioned lineage. A still image cannot establish observed temporal frequency or faithful six-axis reconstruction. No labels, training or qualification are inferred from synthetic controls.

## Evidence

The preceding [native runtime experiment](../../docs/PULSAR_SEMANTIC_IMAGE_RUNTIME.md) supplies a pinned candidate and two simple pixel-control results. Those controls alone do not prove this Engine worker implementation, model motion quality, installation, release or physical/human acceptance. A separate source-bound checkpoint will record this increment's tests and native custody checks.

Worker foundation evidence: [implementation record](../../docs/PULSAR_SEMANTIC_IMAGE_WORKER.md). Native custody and transport checks pass; the candidate fails both authored shape-to-pattern instruction controls. No model qualification follows from the worker checks.
