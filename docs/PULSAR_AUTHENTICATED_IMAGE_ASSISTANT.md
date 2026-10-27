# Authenticated prepared-image assistant

Status: software increment implemented and tested; roadmap PARTIAL; unqualified.

An authenticated client can request `PrepareAssistantImage` against an imported media source at the current project revision. The confined Preview worker requires exactly one decoded frame and produces packed RGB8 with a maximum of 512 pixels per axis. It accepts no model, view, placement or annotation in this mode. The Engine issues a pathless reference binding project/session/revision, original source, normalized pixels, actual ffmpeg/ffprobe identities and preprocessing version.

Trusted `Engine::register_vision_assistant_profile` registration declares the exact projector identity and private host path. `AssistantStart.image` selects an exact issued reference, and only creative synthetic scope can use it. Saved reports retain the reference. Start, completion and creative approval recheck source membership and both original/derived bytes. Worker completion must bind the original projector, pixels and dimensions. The fresh context labels an image as a theme and explicitly carries `observed_temporal_motion: false`.

The running Engine retains at most 32 prepared references. A full output-ceiling storage claim is acquired before decoding; deletion failure retains both the owned directory and its claim and refuses further preparation until retry succeeds. Successful eviction deletes only the Engine-created attempt directory. Restart and eviction expire references. Shutdown cleanup is best effort; crash recovery and persistent-disk performance are separate unqualified gates.

Existing text requests omit the optional image/projector fields and preserve their encoding. Existing constrained creative approval still uses the original approval request as the receipt/fingerprint subject. No proposal can directly commit a master program or move a device.

## Verification

| Check | Result |
| --- | --- |
| Protocol library | 129 PASS |
| Default Engine library | 838 PASS, 67 ignored |
| Client library | 253 PASS, 5 ignored; state/logic tests, not real GUI qualification |
| All-feature Engine library | 852 PASS, 93 ignored |
| Authenticated native still/vision path | One PASS in 86.66 seconds; actual worker and pinned media/runtime/model/projector |
| Strict workspace/all-target Clippy | Default and all features PASS |
| Default workspace application/worker build | PASS |
| Format, whitespace and structure guards | PASS; 403 structure checks and 36 guard tests |

The native check imports an authored single-frame P6 still, verifies exact decoded RGB, rejects forged metadata and wrong owner/project/revision, runs a real registered vision profile, reads the completed bound report, and rejects subsequently changed original and derived bytes. Proposal alone leaves the master and jobs unchanged. A separately authored report fixture tests approval derivation and stale-byte refusal; it is not a learned model result.

The first Engine run passed 835 and failed two profile-budget fixtures because the harness omitted its explicit ONNX runtime; the corrected run and final suite pass. Two initial test-build failures were fixture mistakes and were corrected without widening production visibility. The initial missing client response mapping was also corrected. Negative logs are retained in the checkpoint. Library tests use the RAM-backed, network-isolated harness; persistent-disk performance and durability are not qualified by these runs.

## Native resource regression

Three attempts at the full authored request failed before completion. The bounded diagnostic identifies SIGKILL in the native runtime. A read-only observation of that runtime records a 122-second hard process CPU limit, a last sampled 120.03 CPU seconds, peak cgroup memory of 770,076,672 bytes and zero OOM counters. The old confinement code derived process CPU seconds from wall time alone, although Linux sums CPU time across all process threads.

The correction rounds wall milliseconds up to seconds, multiplies by the declared thread count, then adds the existing two-second margin. Concurrent CPU remains bounded by the cgroup and wall time remains bounded by the supervisor. The original native request now passes. Its final sampled CPU time is 147.15 seconds under a 242-second limit, with 770,097,152 bytes peak cgroup memory and zero OOM counters. This exceeds the old limit while completing inside the wall deadline. Temporary diagnostic instrumentation has been removed; no native diagnostic is placed in production responses.

## Remaining work and qualifications

[Owner policy setup and CLI image preparation](PULSAR_VISION_OWNER_POLICY_CLI.md) are now implemented and tested. [Desktop image preparation and saved-request review](PULSAR_DESKTOP_IMAGE_ASSISTANT.md) are implemented and controller-tested; interactive GUI qualification remains UNKNOWN. Dedicated image-conditioned generation lineage remains open. Existing prompt-only image generation is unchanged. The prepared catalog is ephemeral. The upstream multimodal CLI remains an experimental candidate rather than a selected/distributed default.

The preceding candidate's two shape-to-pattern instruction controls remain FAIL. Authored control images are not user reference media, annotations, training or useful-model acceptance. No existing annotations were supplied; software work continues without inventing any. Handy 2 Pro remains the requested physical target, with firmware and encrypted keys not established. Real-media accuracy, GUI/platform, physical-device, human and release gates remain open.

Decision: [ADR B3-131](../research/phase-b/ADR-B3-131-authenticated-prepared-image-assistant.md). Preceding worker foundation: [bound image worker](PULSAR_SEMANTIC_IMAGE_WORKER.md).

Exact subjects, harness, observations and negative results: [checkpoint](evidence/roadmap-2026-10-11/authenticated-image-assistant/checkpoint.json).
