# Private CUDA live-stream contract

This increment adds typed declarations and an ordered execution transcript for a future persistent CUDA live worker. It does not enable CUDA live execution. The ordinary live manifest validator and CPU worker remain CPU-only.

`LiveCudaInferenceManifest` binds the existing live capture identity, clocks, attempt, model, runtime, worker, preprocessing and bounded budget to a CUDA execution declaration, dependency membership and a separate evidence directory. Validation requires the captured ONNX Runtime alias, captured NVML dependency and nonoverlapping capture/evidence paths. The manifest has a canonical SHA-256 identity. It is not an offline generation request and carries no invented job or source artifact.

`LiveCudaStreamCommand` and `LiveCudaStreamReply` declare readiness, frame exchange and explicit finalization messages. `LiveCudaTranscript` hashes successful exchanges using a domain-separated, length-delimited chain. Each exchange includes issued frame/pixel identities, clocks, position and analysis duration. Sequences must be contiguous; current frame ordinals and rational source times must increase. Empty, replayed, invalid, exhausted or poisoned transcripts cannot finish. Finalization consumes process-local state and records the manifest identity, run count, last frame/time and transcript hash. The bound is 16,384 successful runs.

A completed exchange remains in the transcript even when the independent freshness fence rejects its output. A worker-authored summary is insufficient: the future parent must compare its own expected transcript and independently verify the original native trace and receipt against the same run count and declaration. This metadata does not prove GPU execution or grant motion/device authority.

## Verification

Six focused protocol tests cover dependency/path admission, an independent hash-chain oracle, ordered clocks and pixel declarations, replay/poisoning, run exhaustion, result/timing binding and unchanged ordinary CPU serialization. A frozen preceding CPU reader and the current reader are used for compatibility checks: both ordinary modes must accept identical canonical CPU bytes and reject CUDA declarations. The current reader's explicit CUDA check mode validates metadata only.

The [evidence packet](evidence/roadmap-2026-10-07/live-cuda-contract/) retains exact source archives, raw logs, reader identities and terminal verification. The terminal gates passed: 982 workspace tests (51 ignored), 514 CUDA-feature tests (48 ignored), five shipped CPU application cases, one reader compatibility case without skips and 251 structural checks. All 338 live/archive source records and 66 executable identities match. See [ADR B3-052](../research/phase-b/ADR-B3-052-live-cuda-stream-contract.md).

## Required activation work

The next implementation must connect supervised native capture, fresh interactive GPU memory admission, a distinct verified loader/stream role, inherited absolute lease deadlines, bounded original receipt/trace egress and retained image/device/resource custody through exact retirement. It must not reconstruct admission from declaration metadata or repurpose an offline job to launch a live stream.

Persistent temporal stroke transport is only a prerequisite. Required fastest-preset live six-axis generation, measured latency, independent model/media quality, physical Handy testing, platform qualification and human/release acceptance remain open.
