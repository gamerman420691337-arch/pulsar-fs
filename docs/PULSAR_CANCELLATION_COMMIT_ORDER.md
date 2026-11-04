# Worker cancellation after durable commit

`CancelJob` must commit its job/attempt state and exact replay receipt before signalling the worker. A receipt-write or commit refusal must roll back without causing worker cancellation. The engine captures the existing worker flag while holding the database transaction; after successful commit it signals that exact flag before releasing the database mutex. There is no fallible lookup between commit and signal and no signal on historical replay.

This preserves protocol, database, portable format, authority and response semantics. It does not release resources early or prove worker termination. Committed cancelled state still fences late publication. A crash after commit but before signal leaves durable cancellation; it does not establish that the process stopped. Existing worker retirement and startup recovery remain separate obligations.

Scope is the direct `CancelJob` handler. The [grant revocation follow-up](PULSAR_REVOCATION_COMMIT_ORDER.md) now captures generation/preview/WASM handles and signals only after its transaction commits. [Direct WASM cancellation](PULSAR_WASM_CANCELLATION_COMMIT_ORDER.md) also has its own corrected ordering. Other effectful transaction paths and broader runtime/resource qualification remain separate work. Catalog approval withdrawal and automatic cancellation of already accepted pack work remain open.

[Source-bound evidence](evidence/roadmap-2026-10-10/cancellation-commit-order/checkpoint.json) on dirty branch `codex/pulsar-roadmap-m1`, base `4da4c08b59dc7b90e6c5aa3b420908257194eb8d`:

- The first default-storage regression probe exceeded its 30-second limit and kill grace (exit 137), with no verdict. The same real handler/SQLite injection under `TMPDIR=/dev/shm` reproduced the pre-fix worker signal after rollback (exit 101, 0.01 seconds). This is transaction-order evidence, not disk-durability evidence.
- The corrected regression covers receipt insertion refusal and deferred foreign-key refusal at COMMIT, unchanged job/attempt state and absent receipt after rollback, explicit same-ID retry, and historical replay that does not signal a replacement handle. PASS, 0.01 seconds on tmpfs. Existing late-publication fencing also PASS on tmpfs, 0.01 seconds.
- The complete all-feature engine library with ordinary temporary storage PASS: 717 passed, 79 ignored, 83.29 seconds. This includes the new regression on ordinary storage. No ignored native gate or release qualification is inferred.
- Workspace strict all-target Clippy PASS with default and all features; formatting, 350 architecture checks and 36 source guards PASS. Whole-workspace runtime was not rerun; existing client journal/storage gates remain unresolved.

Only `crates/pulsar-engine/src/authority.rs` changed among the preceding checkpoint's 348 hashed source paths. The packet retains the exact incremental patch, before/after file, source/binary identities and raw public logs. Full model/media/device/platform/rendered/human/release roadmap remains incomplete.
