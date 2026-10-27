# Durable CUDA attempt intent and schema-15 barrier

The engine now has an immutable CUDA attempt-intent ledger. Declaration binds
one canonical validated worker manifest, its SHA-256, job/attempt identity and
engine epoch. The manifest retains model/runtime/policy, captured-library and
device declarations plus execution budgets. The ledger is intent, not evidence
of owner approval, reservations, captured bytes, mounted devices or launch.

Declaration runs inside the existing job/attempt transaction after the exact
queued rows are inserted. It publishes the ledger and schema-15 reader barrier
together. Its own savepoint rolls back a failed declaration without discarding
caller work. An exact queued replay in the same epoch is idempotent; replacement
or replay in another epoch refuses. A new admitted attempt must receive a new
identity and independently establish its current authority and resources.

CPU operations do not create this table or advance to schema 15. Startup audits
CUDA membership through the read-only SQLite probe before writable/WAL startup.
It checks layout/foreign keys, scalar and record bounds, canonical request/hash,
immutable attempt membership, current-job binding and project/source/revision
scope. CUDA manifests in older schemas or without matching ledger membership
refuse. Placement and imported-interval migrations now preserve schema 15;
subsequent ordinary pack selection/deactivation cannot lower it.

Startup marks queued/running work interrupted, while retaining the immutable
GPU epoch and manifest. GPU intent is excluded from automatic dependency-based
resume. Persisted intent does not reconstruct GPU accounting or reauthorize an
old process. Explicit GPU preparation still refuses before creating an attempt
because trusted mounted custody and production launch remain unimplemented.

## Bounds and evidence

Each record is at most one MiB. The ledger admits at most 4,096 records and
64 MiB of manifest bytes; identifiers, hashes and epoch scalars have independent
type/byte bounds. Oversized data refuses before Rust string decoding. These are
data admission bounds, not measured SQLite RSS, startup latency or whole-device
VRAM limits. Audit cost scales with retained history. Ledger archival/retention
qualification remains open; exhausted capacity does not silently delete intent.

The [validation packet](evidence/roadmap-2026-10-01/cuda-durable-intent/validation.json)
retains exact source archives, commands, compiled identities and database/WAL
readbacks. Unit cases cover CPU legacy behavior, transactional rollback,
idempotence, epoch replacement, restart, layout, hidden/missing membership,
corruption, scalar bounds and independently exceeded row/aggregate caps.

The actual current engine/CLI fixture uses an authored three-action script and
inserts declared GPU metadata with a private test helper while the engine is
stopped. This is not production admission or native inference. The exact
retained schema-14 engine refuses; the current engine preserves motion, leaves
GPU work interrupted and keeps schema 15 across restart and pack deactivation.
Twelve corrupted variants refuse. All refusal cases hold committed nonempty
WALs and compare main database/WAL hashes before and after. The first fixture's
absent-to-empty WAL observation is retained as history, without an archived
failed source subject, rather than being treated as changed committed data.

The final current-source campaign passes 897 Rust cases (seven qualification
cases ignored), 436 CUDA-feature engine cases (one ignored), all 97 configured
JavaScript cases without skips and 221 static structure checks. Thirty-five
executable/spec identities are unchanged. An initial full campaign exposed the
older unknown-future-schema fixture's hardcoded version 15; it now uses 16,
while the dedicated CUDA fixture checks known schema 15 with missing layout.
A subsequent campaign timed out during the existing interrupted-pack retry.
Its copy was nearly complete; the same source and executable passed the
isolated case and final full rerun without a timeout change. The cause remains
unproven. Both failed campaigns retain their exact source and gate subjects.

## Remaining integration

Complete fresh owner/resource admission, durable captured-image custody, held
device mounts, confined GPU process launch, accepted result/trace membership and
retirement/restart fencing. The ledger deliberately cannot grant any of them.
No production GPU jobs or physical devices are actuated by these tests.
Model/media accuracy, Handy, platform, performance, human and release gates
remain independent and open.
