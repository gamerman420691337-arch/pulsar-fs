# Native capture pressure, 2026-10-09

Status: PARTIAL. Read-only pressure observations and bounded budget comparison; production configuration unchanged.

## Actual live capture

The [packet](evidence/roadmap-2026-10-09/native-capture-pressure/checkpoint.json) uses the source-matching CUDA release SHA4838d0f9027092d08ba38df7814e54065a8409cd275e5a0713c3185718eb3909. The native test now records bounded cgroup ancestry values for descendants of its owned Engine PID and the capture retirement reason. It logs no environment or command-line values. The initial capture FAILS before Ready and is explicitly observed `expired` at about30.22s. Shared-image handoff is not exercised.

The native producer's memory.max is536,870,912 bytes and memory.high ismax. Sampled memory.current peaks at536,834,048. Its final sampled memory.stat has anon901,120 bytes and file530,354,176 bytes, so file cache dominates the observed memory footprint. pgscan/pgsteal reach645,865/645,845. The worker's fresh memory.events reports max8192, high0 and oom/oom_kill0; cpu.stat reports no quota throttling in the sampled interval. Fifteen of28 worker samples wait in dirty-page throttling, with other file/page/journal waits. At29.19s it has written about1.254GB and is still in native capture.

There is no explicit io.max entry in the sampled worker/ancestor groups, and ancestors have no finite memory.max or memory.high. Parent event counters are hierarchical and historical; their old OOM entries do not establish an OOM in this run. One host snapshot also records nonzero global I/O pressure. This is not a sole-cause claim.

The interpretation of file cache accounting and max events follows [Linux cgroup v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory-interface-files): file includes cached filesystem data; max counts attempted crossings of the memory boundary. A limit event does not by itself prove the cause of a deadline failure.

## Standalone comparison

A scratch copy of the existing supervised-native-capture-check example accepts a sixth memory-budget argument and uses a2GiB parent pool with one worker slot. It links a directly compiled current Engine rlib against exact cached default dependencies; only the diagnostic wrapper differs. The actual confined child is the same source-matching CUDA release for every run. CPU1, address-space1536MiB, full75-library1,945,994,000-byte declaration and60s inspection deadline remain constant. This is not a live capture or release qualification. The native protocol caps memory at1GiB, so no2GiB capture is attempted.

| Order | Native budget | Capture result | Capture time |
| --- | --- | --- | --- |
| 1 | 512MiB | Deadline refusal | 60.00s |
| 2 | 1024MiB | Captured; independent full-byte readback | 24.11s |
| 3 | 1024MiB | Captured; independent full-byte readback | 30.29s |
| 4 | 512MiB | Captured; independent full-byte readback | 59.30s |

All four inspectors exit0 and confirm cleanup and storage_after_cleanup=0. Each successful result retains the declared byte count, completion identity, storage charge before close and independent readback. Both budget sizes reach their memory cap and report max events with no OOM; raw counts are not directly comparable across different amounts of completed work. Larger budgets have shorter observed capture times in both orderings, but cache state and shared host load are not controlled. Four serial observations do not establish a stable latency distribution or sole causal effect.

Notably,30.29s already exceeds a30s live lease before the memory-observer and inference bootstrap stages. The other larger-budget result also leaves limited preparation time. No production memory budget, lease, global dirty limit, scheduler, WBT setting or shared service is changed.

## Source, checks and next decision

The source checkpoint binds415 records: one test-only JS change,414 unchanged. Rust production is identical to the previous restored source. Architecture/guards/syntax pass289/36/one. Diagnostic parent and inspector compile logs, wrapper source, comparison commands and pressure samples are retained. Owned compiled outputs and all run roots are removed after confirming final custody and storage release.

The next implementation decision should explicitly account for the native image's file-cache pressure within aggregate admission and the protocol ceiling, then verify actual cold Ready and overlapping handoff with fresh byte proofs. A larger admitted preparation budget is a measured candidate, not a demonstrated complete fix. If it cannot consistently fit the existing lease, test a copy method that avoids dirty-page accumulation under the existing budget before adopting it. Preserve all checksum, storage, worker retirement and authorization invariants.

Unresolved workspace failures, sustained encoded/camera/live six-axis capture-to-command performance, native actor/revocation interleavings, trained model/real-media quality, installed Stash, offline/platform coverage, physical Handy and human/release acceptance remain open. Full roadmap objective unchanged.
