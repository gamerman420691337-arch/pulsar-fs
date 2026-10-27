# Engine and client regression checkpoint

Status: scoped software regression PASS on private tmpfs; ordinary-storage suite INCOMPLETE; full roadmap PARTIAL.

The existing 370-source subject remains byte-identical to the preceding capacity-recovery checkpoint. Default Engine library tests PASS 761/zero failed/59 ignored in 24.50 seconds. Separately built all-feature Engine tests PASS 775/zero failed/84 ignored in 32.00 seconds, and all-feature client library tests PASS 218/zero failed/four ignored in 7.71 seconds. Actual binary paths come from Cargo output. Each complete suite runs serially in a separate Bubblewrap mount namespace with a two-GiB private tmpfs fixture root; normal storage elsewhere is read-only. No ignored native/device cases were enabled. These are complete named library suite verdicts, not full workspace, rendered desktop, process-launch, native model or release gates.

The ordinary-storage default Engine library invocation TIMEOUT124 at 360 seconds, after some early assistant tests passed and while generation-job/original-receipt coverage was pending. It produced no completed suite verdict or assertion failure. A bounded live process observation records the main process in futex_wait; this alone does not identify the blocked test thread, filesystem, lock or underlying cause. Do not promote private passing suites into ordinary-storage repair or load-independent reliability. Prior failures remain attached to their original subjects.

No Rust, protocol, model, runtime or application binary behavior changed. The newly completed [milestone ledger](PULSAR_MILESTONES.md) resolves the artifact named by the major roadmap and keeps every M1–M8 qualification gate open until its own evidence exists. No labels, benchmark results, physical device observations or human signatures were fabricated.

[Source/binary/environment-bound evidence](evidence/roadmap-2026-10-11/engine-regression/checkpoint.json) retains commands, completed verdicts, timeout output and process metadata. No real media, sterile/live training references, user Stash service, hardware or keys were accessed.


Subsequent [ordinary-storage diagnosis](PULSAR_STORAGE_SYNCHRONIZATION_DIAGNOSIS.md) reproduces an 11.64-second fsync stall outside Pulsar/SQLite, while the previously stalled exact fixture passes in 0.29 seconds. A serial default suite still times out at180 seconds; journal-wait thread snapshots and individual sync timings are retained. No production repair or full-suite verdict is claimed. All370 source/config hashes and the default binary remain unchanged; full roadmap PARTIAL.
