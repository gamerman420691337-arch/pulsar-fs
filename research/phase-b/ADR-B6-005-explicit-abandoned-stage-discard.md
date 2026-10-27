# ADR B6-005: explicitly discard one recognized abandoned pack stage

Status: implemented locally on Linux; broader recovery and release qualification remain open.

## Decision

Provide a distinct authenticated mutation bound to exact stage basename, captured manifest SHA and retained request ID. Reacquire the marker lock and validate ownership, private modes, the entire declared partial tree and published inode exclusions before mutation. Reject legacy, locked, aliased, unlisted or changed contents. Point-in-time observations never authorize cleanup.

Reserve the authority request identity before filesystem effects. Persist immutable intent, move without replacement to request-bound quarantine, remove only identity-checked entries through directory descriptors and persist completion. Exact completed replay survives restart. Existing partial quarantine can resume. Missing original/quarantine after intent without completion reports uncertain absence; it cannot fabricate removal. Counts describe the initial logical plan, not physical freed space. Corrupt records refuse.

## Boundaries

Filesystem records are retained; new-intent admission stops at 256 existing record entries. No automatic retention/GC, quota, legacy repair, active pack removal/update/rollback or publisher authority follows. Advisory cooperation and private state remain assumptions. No runtime loads or model activation occur. Power-loss, disk-full, exhaustive deletion crash points and non-Linux adapters remain unqualified.

## Evidence

See [documentation](../../docs/PULSAR_OFFLINE_MODEL_PACKS.md#explicit-abandoned-stage-discard) and [source-bound local gates](../../docs/evidence/roadmap-2026-10-01/pack-stage-discard/validation.json). Native tests use an actually crashed authored partial import, then exercise refusal, exact discard and completed replay across another actual engine restart. Unit cases synthesize partial quarantine and missing-completion states. No actual user-state cleanup or physical Handy qualification occurred.
