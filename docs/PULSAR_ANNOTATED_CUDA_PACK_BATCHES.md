# Annotated CUDA model-pack Stash batches

Owner-selected annotated CUDA model packs now have authored native batch evidence for both full-source generation and per-source placement/interval generation. This checkpoint changes native verification scripts only: the preceding Engine implementation already composes the exact selected pack, per-source view, acknowledged placement and CUDA execution path. No new provider authority, wire shape or storage migration is introduced.

Use the existing `model pack-import`, `model pack-select --approve-unqualified-native-execution` and `stash generate-missing --model-pack` controls with an annotated CUDA profile and exact source catalog. The Engine requires its private owner-configured CUDA policy and matching pinned runtime. Optional [source plans](PULSAR_STASH_SOURCE_PLANS.md) supply explicit placement, project interval and source-time publication intent. Every job must still recheck current owner selection and retain the exact pack manifest, profile and approval document dependencies. Selecting a pack approves unqualified native execution only; qualification flags remain false.

## Native correspondence and recovery

Two campaigns run the shipped CLI/local IPC against private Engine states and their own query-only loopback Stash fixtures. Each generates two different authored stereo sources with different selected eye/target declarations, restarts after durable generation intent, and preserves exactly one generation request per source. A third wrong-hash source stops before any generation request/job/output. Withdrawn owner selection and stale-ledger migration refuse. Explicit replacement using the new selection revision stays paused through restart and replay, then completes only after separate resume.

A fourth source is held during capture while owner selection is withdrawn. Its later generation creates no job or sibling script and retains `needs_attention`; restart preserves that outcome. The database contains exactly the three completed jobs. All accepted jobs retain exact pack-manifest, profile and approval dependencies, including the repaired job's new approval revision.

Across six completed CUDA attempts, every original receipt records 27 successful model runs; each trace contains 324 computation nodes, all assigned to `CUDAExecutionProvider`. Both annotated-generation and CUDA evidence memberships survive restart. Exact original receipt/trace hashes remain unchanged. All 84 checked rounded positions match the independent Python model fixture; the maximum unrounded difference is 2.9802322387695312e-8.

The placed campaign uses forward rate 2 and reverse rate -3 with different nonzero project anchors, exact immutable source placements and explicit project intervals. All three publications retain source-time correspondence proofs and reproduce the 28 original script timestamps from 0 through 3375 ms. Native assertions and an independent Python fraction check verify all 84 sample project times from original source PTS/time bases and retained placement specifications, including subnanosecond quantization. These single-placement proofs do not establish mixed-clock piecewise publication.

Both corresponding native CPU campaigns pass on the same CUDA-enabled executable with their CPU runtime. All four campaigns end with empty worker custody and terminal private processes. Syntax checks, 288 architecture checks and 36 source guards pass. Rust/Cargo suites are not repeated because no Rust, Cargo, fixture or production source changed; their source-bound preceding results remain 1,125 workspace tests (63 ignored) and 601 CUDA Engine tests (68 ignored). The preceding workspace's intermittent lease reacquisition failure remains recorded and unresolved.

## Reproduction and evidence

Run `node --test --test-concurrency=1 scripts/stash-annotated-pack-batch.test.mjs scripts/stash-annotated-placed-pack-batch.test.mjs` with `PULSAR_STASH_TEST_BINARY`, `PULSAR_ANNOTATED_MODEL_FIXTURE_DIR` and `PULSAR_ORT_DYLIB`. Supply `PULSAR_ANNOTATED_BATCH_CUDA_POLICY` for CUDA; omit it for CPU. Missing native inputs skip these campaigns and provide no native evidence. The policy's entire library closure is checked against its declared lengths/hashes before the campaign starts.

The [source-bound checkpoint](evidence/roadmap-2026-10-09/annotated-cuda-pack-batches/checkpoint.json) retains 406 source records, exact source archive readback, original authored requests/programs/receipts/traces, memberships, terminal custody and independent clock checks. The executable is byte-identical to the preceding passing CUDA file-model batch checkpoint. No new preceding-reader refusal is claimed.

## Remaining work

[File-model CUDA placed/interval batches](PULSAR_ANNOTATED_CUDA_STASH_BATCHES.md) now have successor native evidence. Annotated CUDA preview and desktop workflows, sustained performance, useful held-out model quality and real annotations, mixed-clock piecewise publication, installed Stash, broader fault/retention and offline/platform qualification, physical Handy playback and human/release acceptance remain open. No fitting, live/sterile media access, user Stash contact or physical actuation occurred. Roadmap status: PARTIAL.

The subsequent [selected CUDA pack desktop checkpoint](PULSAR_DESKTOP_CUDA_PACK.md) verifies authored pack preview/generation/commit/export and withdrawal/historical-replay/restart behavior through actual window controls. Placed/interval desktop combinations and sustained performance remain separate open gates.
