# Downloading original retained CUDA evidence

Use `pulsar live-capture download-evidence PROJECT CURRENT_REVISION CAPTURE_EPOCH PART OUTPUT`. PART is one of `origin`, `manifest`, `summary`, `receipt`, or `trace`. The command works after restart with a CPU-only reader and no CUDA owner policy. It never replaces an existing destination.

A new Begin requires current project Read authority and current expected revision. The Engine validates retained original correspondence through the shared bounded reader, then grants an existing opaque download lease for that exact immutable part. Every bulk operation still checks current authentication, Read grant, boot, expiry and admitted range. These historical bytes confer no inference, capture, motion-review or device authority.

The lease retains original evidence and its memory admission through completed exact retries. Chunk memory also remains charged until retirement. Abandonment, revocation and expiry release these claims. Completed evidence downloads count against aggregate lease admission until released. Original bytes remain in RAM; no new Engine staging files or recorded paths are used. The existing bulk wire layout, worker format and database schema are unchanged. Restart invalidates active leases under the existing epoch and request journal rules.

The client streams bounded chunks, verifies length and SHA-256, syncs verified staging, abandons the lease, and publishes through the existing held-directory guard. Truncated/corrupt bytes, wrong project or endpoint, failed requests and occupied destinations cannot publish a partial or replacement file. Publication remains unsupported where that guard is unavailable.

The actual shipped CPU-only CLI downloads all five originals after reopening authored CUDA evidence. Each output and returned digest matches the original SQL blob and preceding retained file byte for byte. Cancelled and stale requests refuse, an existing canary remains unchanged and no client staging directory remains. Independent readback matches five abandonment receipts to their exact durable Begin lease IDs. Its initial assumption that the entire reused database had only five begins was incorrect: three earlier motion downloads also remain. The failed assumption and corrected readback are retained; production bytes did not change.

See [ADR B3-068](../research/phase-b/ADR-B3-068-retained-live-cuda-download.md) and [the source-bound checkpoint](evidence/roadmap-2026-10-08/live-cuda-evidence-download/checkpoint.json).

Listing/retention controls, desktop presentation and additional concurrent eviction/restart/disconnect/read fault campaigns remain open. Broader capture races, sustained acquisition, unresolved startup variability and parallel failures, model/media/multimodal/six-axis/performance/Stash/assistant work, actual Handy/platform/human acceptance and Preview/Stable release qualification remain open.

Verification: the final serial workspace gate passed 1,040 tests (55 ignored), the CUDA Engine gate passed 554 (63 ignored), the shipped CPU-only CLI case passed, and all 268 structural checks passed. All 366 source records match the current files, saved snapshots and archives. The interrupted initial CUDA observation and its completed retry are retained separately. These are scoped software checks; Phase B and release qualification remain open.
