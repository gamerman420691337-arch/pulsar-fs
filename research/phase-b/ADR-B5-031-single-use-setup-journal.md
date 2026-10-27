# ADR B5-031 — Claim setup durably before transport handoff

Extend the device effect journal to version 2 with setup records and a unique live route
reservation. A prepared record is not a dispatch claim. The claim commits before the
non-serializable token is returned; repetitions and restarts cannot obtain another token.
The boot owner fences old writers, and only unclaimed preparation can be cancelled locally.
Acknowledged and uncertain attempts keep their reservation until a separately implemented
reconciliation proves it can be released. An acknowledgment is not loaded-program authority.

Version-1 play/stop rows remain unchanged during the transactional upgrade. Prior readers
refuse version 2. Durable records contain identities, not credentials or hosting URLs; recovery
provides history only. Callers must independently validate current qualification, controller
authority and credential-route identity before preparing and claiming. Public setup dispatch,
loaded-program reconciliation and actual device qualification remain separate integration work.

Bind actor/request identity independently of the boot and payload hash, and include the
hosting request in the subject. Cancellation cannot permit request rebinding. Read-only
startup validation precedes writable SQLite: the prior reader's refusal was observed to
checkpoint WAL bytes without changing logical rows. Preserve that finding and verify the
current reader against a committed future-version WAL; do not claim older byte preservation.
