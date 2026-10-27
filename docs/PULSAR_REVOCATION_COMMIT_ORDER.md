# Grant revocation commits before worker signals

Revocation must persist its grant deletion, job/attempt and WASM state changes, event and exact replay receipt before signalling generation, preview or WASM worker handles. The transaction captures the exact scoped handles, then signals them after successful COMMIT while still holding the database mutex. A failed transaction does not signal those workers. Historical replay does not signal newly registered handles.

Protocol, storage and portable formats remain unchanged. Signals request cancellation; they do not prove worker termination or release resource claims. Committed state and fresh authority checks continue to fence late publication. A crash between COMMIT and signal retains durable revocation without establishing process retirement. Existing transfer/package post-commit revocation remains separate; an error there cannot undo committed authority withdrawal.

[Source-bound evidence](evidence/roadmap-2026-10-10/revocation-commit-order/checkpoint.json) on dirty branch `codex/pulsar-roadmap-m1`, base `4da4c08b59dc7b90e6c5aa3b420908257194eb8d`:

- The real authorized Revoke handler with SQLite receipt-write refusal reproduced signalling after rollback before the fix: FAIL exit 101, 0.01 seconds on tmpfs. The corrected case tests receipt-write and deferred foreign-key COMMIT refusal, unchanged grants/jobs/attempts/WASM state/events, absent receipt, explicit same-ID retry, unrelated-session noninterference and historical replay leaving replacement handles untouched. PASS, 0.02 seconds on tmpfs, and PASS within the ordinary-storage full run.
- The stored WASM invocation and worker flags are authored inert fixtures. No guest executes and no physical or actual process-retirement behavior is qualified by them.
- Complete ordinary-storage all-feature engine library: FAIL exit 101, 717 passed, one failed, 79 ignored, 136.93 seconds. `live_staging::tests::expired_post_reply_producer_retains_capacity_until_join` failed with **“retirement did not release claims”**. The unchanged case passed alone in 5.16 seconds. That does not establish repair or current full-suite PASS.
- Default/all-feature workspace all-target strict Clippy PASS; formatting, 350 architecture checks and 36 guards PASS. Whole-workspace runtime was not rerun; existing client journal/storage gates remain unresolved.

Only `authority.rs` and `wasm_edits.rs` changed among the preceding checkpoint's 348 hashed source paths; the other 346 hashes match. Raw logs, incremental patches, before/after files and source/binary identities are retained. No deadline, sync setting, unrelated service or model/device authority changed.

Scope is grant revocation. [Direct WASM cancellation](PULSAR_WASM_CANCELLATION_COMMIT_ORDER.md) now has its own corrected ordering and typed rollback/retry verification. Automatic model-pack approval withdrawal still needs separate completion. Full model/media/device/platform/rendered/human/release roadmap remains incomplete.
