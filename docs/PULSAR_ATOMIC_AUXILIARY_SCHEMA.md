# Atomic auxiliary startup schema

Status: **IMPLEMENTED / focused checks PASS / full roadmap PARTIAL**.

Engine startup previously created six auxiliary tables and migrated two columns through separate commits. If the last column migration failed, earlier tables and the earlier column remained published. The existing startup block now lives in `initialize_auxiliary_schema`, which uses one immediate SQLite transaction. Its six table additions and two column additions commit together, or roll back together on an error. Core schema, placement/motion migrations, interrupted-job recovery and file creation remain separate. This does not claim that the entire Engine startup is one atomic operation.

The read-only schema/metadata preflight remains before writable startup. WAL/FULL settings, busy timeouts, file syncs, device-journal ownership and worker deadlines are unchanged. Existing rows, defaults, reader version and schema names are preserved. Only `authority.rs` changed among 422 recorded source paths; 421 are unchanged and no source module was added.

## Verification

The failing regression exercised the extracted existing production initialization block through an in-memory SQLite connection. Omitting the final `exports` table caused a late migration failure and left six newly created tables committed: `left: 6`, `right: 0`. The neighboring preservation/default test passed. After grouping the operations in one transaction, both tests pass in default and CUDA configurations: no auxiliary table or candidate-column addition survives the same failure, and the connection returns to autocommit; successful initialization preserves existing rows/defaults and remains idempotent. This is a real SQL failure/rollback test at the shared initialization seam, not a full malformed-project startup or power-loss qualification.

Default and CUDA compilation PASS. Default targeted checks: two new group tests, two existing placement migration/refusal tests and three existing model-selection reader-fence tests PASS. CUDA targeted checks: both new group tests PASS. Architecture: 294 checks PASS. Source guards: 36 tests PASS. Full Engine/workspace suites, platform isolation, crash/power-loss qualification and release packaging were not rerun for this source.

The actual mixed-clock worker/export regression against the changed Engine reached fresh startup and generated producer output, then timed out at the 480-second outer deadline (485.20 seconds observed). Samples retain test-thread filesystem journal waits. It emitted no completed export verdict. Exact timing, publication observation and the four rehashed proof controls remain UNVERIFIED. Fewer explicit startup transaction boundaries are a source fact; no startup speedup or repair of host disk stalls has been measured.

[Commands, terminal output, before/red/current source, source records, process samples and binary identities](evidence/roadmap-2026-10-09/atomic-auxiliary-schema/checkpoint.json) are retained. Scratch was removed after checking that no process executable, current directory or file descriptor referenced it. Shared services and caches were untouched. No reference training, real-media qualification, device actuation or human acceptance occurred.
