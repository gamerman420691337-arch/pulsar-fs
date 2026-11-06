# ADR B5-008: authenticated lease-owned inference consumption

Date: 2026-10-04
Status: Implemented local integration; broader live/device qualification open

The persistent worker primitive must be owned by the same capture identity and authority that owns pixels. Add optional explicit model/profile admission to capture start. Commit the start receipt and publish the lease before spawning an analysis thread; distinguish starting, running, failed and retired state. Replay current state without rearm. Keep filesystem/native work outside database/registry locks.

The thread reserves aggregate model-worker admission, snapshots exact worker/model/runtime bytes and owns one persistent confined worker. Sweep authority and lease invalidation before taking work and accepting completion. Bind results independently to attempt, capture/project/revision/source, profile digest, original pair and actual pixel hashes. Use the pixel runtime's sealed owner and server monotonic age check to accept or discard completion. Mismatches and worker failures retire, never retry silently. Entry drop/retirement signals cancellation; native watchdog and handle cleanup own process reap.

Retain one latest ephemeral accepted estimate and bounded counters/queue observations. Recompute its freshness when status is read and clear it on retirement. Keep original acceptance age separate from current age, pair analysis duration, presentation lateness and physical response. No per-frame durable receipt, project motion mutation, player buffering or device dispatch is introduced.

Actual native CLI and authority regressions exercise the composed path, ageing, late/revoked/changed-revision discard, mismatch retirement, asynchronous byte failure and replay without another attempt. [Validation checkpoint](../../docs/evidence/roadmap-2026-10-01/authenticated-live-inference/validation.json). Real source acquisition, calibrated clocks, renewal/continuity, qualified models/fallback, GPU fastest-preset six-axis scope, Handy hardware and platform/human/release gates remain required. Schema/portable/Stash proof versions and frozen training/sterile evaluation are unchanged.
