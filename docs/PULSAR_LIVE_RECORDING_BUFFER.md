# Engine-owned live inference recording buffer

2026-10-13. Implemented bounded ephemeral recording/readback; candidate creation,
project commit, persistent recording and funscript export remain unfinished.
Full roadmap PARTIAL.

The Engine can now retain accepted live estimates before the ephemeral `latest`
slot is replaced or cleared on capture retirement. It captures estimates at the
same acceptance boundary that checks lease-issued work, attempt/profile, pixel
hashes, clock and source identities. Stale/rejected results are not appended.
CPU inferred and CUDA provisional result kinds remain distinct and unqualified.
No client-supplied position or guessed acknowledgement becomes recording evidence.

## CLI

Start a normal authenticated live capture with an explicit inference profile, then
start recording before its first inference completes:

```text
pulsar live-capture recording-start PROJECT REVISION EPOCH --maximum-samples 1024 --request-id ORIGINAL_ID
pulsar live-capture recording-page PROJECT REVISION EPOCH --after 0 --limit 64
pulsar live-capture recording-discard PROJECT REVISION EPOCH ORIGINAL_ID
```

Readback is actor/project/revision/boot-bound, including after Stop/Superseded.
The page includes the original recording intent, capture identity, clock, profile,
inference attempt, counts, exhaustion/retirement state and accepted sample proofs.
Pages are bounded to 64 samples. Follow `next` until absent; an active recording
may grow during paging, so stop capture before treating readback as final.

Maximum count is 1..4096, default 1024. Admission reserves 16 KiB per possible
sample from the aggregate Engine memory pool before publication; each serialized
estimate is bounded to 8 KiB. A full/oversize recording stops inference with an
explicit ResourceExhausted failure, retaining the partial samples and exhausted
flag. Completion counters update before failure retirement so status stays valid.

Discard frees payload and reserved memory but retains intent/count/exhaustion
history. Exact original Start replay returns the current recording/tombstone and
cannot rearm a discarded recording. Changed or replacement intents refuse;
wrong release identity refuses. A late Begin after inference completion refuses.
No durable reply with the new page type is inserted into the project database.
The recording is ephemeral to its Engine boot and may disappear with retained
capture eviction. It is not crash/power-loss persistence or secure erasure.

The browser adapter does not yet enable/read these buffers automatically. Linking
recorded scopes across handoffs, durable recording packages, candidate assembly
with explicit missing coverage, review/commit and standard funscript publication
are the next integration work. Existing values upload deliberately remains an
authored edit rather than a route to promote client data into inference evidence.

## Verification

Final selected checks pass 879 Engine library tests (70 ignored), 410 Client tests
(ten ignored), and 290 Protocol tests across its configured executables (none
ignored). Both strict Clippy configurations, Windows source check (existing
warnings), App/Worker/controller builds, formatting, architecture and 36 guards
pass. Core sources are unchanged; no fresh Core suite is claimed.

The network-disabled private-RAM native campaign uses actual CLI/Engine/CPU
inference. Three accepted samples retain exact 0.1/0.2/0.3-second source stamps,
original sequences/scopes and the independent scalar-model position oracle after
Stop. One-sample pages, foreign actor despite project grants, changed intent,
invalid page/release, discard/replay/no-rearm, late Start and capacity failure pass.
The over-capacity recording retains one point with an exhausted flag and a valid
WorkerFailed report. One project, no ordinary jobs/candidates/exports, empty
Worker custody and a joined owned Engine are verified.

Initial compile failures omitted Debug handling for the new response. The first
native capacity case exposed accepted/completed counter mismatch; its failing
status, preceding source/binary identities and corrected red/green campaign are
retained. That native harness overwrote its first Engine suite log with runtime
output; the final rerun separates logs and retains the complete final suite.
These diagnostics do not establish real-media/model accuracy, actual CUDA
recording, browser recording/export, platform/device/human or release acceptance.

See [ADR B3-187](../research/phase-b/ADR-B3-187-live-recording-buffer.md)
and [checkpoint](evidence/roadmap-2026-10-13/live-recording-buffer/checkpoint.json).
