# ADR B3-184: Retain acknowledgement-covered pending metadata without deleting bytes

Status: implemented scoped Linux/Android Client behavior; broader qualification open.

Actual client death can leave a fully written `.pending-UUID` journal beside the
current record. Automatic deletion loses evidence, and adopting matching output
files would replace positive original-request authority with file presence.

Provide explicit path/digest retirement only after the completed current journal
covers the pending file's exact immutable history and request identities. Hold the
private workflow lock/directory/file descriptors, verify bounded bytes and current
record identity, sync, then rename without replacement into a deterministic
retained name. Preserve uncertain outcomes and retry the original path/digest;
observe a matching retained file instead of inferring success from absence.
Share the service between CLI and desktop without Engine generation/export work.
Keep unknown effects and missing-positive outcomes refused.

See [contract and evidence limits](../../docs/PULSAR_LIBRARY_PENDING_RETIREMENT.md)
and [checkpoint](../../docs/evidence/roadmap-2026-10-12/library-pending-retirement/checkpoint.json).
