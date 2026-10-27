# ADR B3-181: existing-directory channel publication with pinned authority

Status: accepted software design; qualification and broader faults remain open.

EDIT-06/TEST-15 require preserving existing channels while filling missing ones.
Whole-bundle exclusion prevents that behavior; a new-directory atomic publisher
cannot merge into an existing user bundle. Path-only publication also cannot
enforce a captured directory identity when the directory is replaced.

Capture the existing directory's device/inode plus missing axes with the media
and recipe, separately from six-channel synthesis. Use schema 6 and reader floor
34. Preserve legacy omitted fields and retain source/output identity through
cancellation and migration. Reject blockers rather than treating them as missing.

Keep all file effects in Engine. Add a project/revision-bound `ExportAxisPinned`
request to the existing export reservation/replay contract. Check the captured
directory and no-replace target before effect admission, then publish an anonymous
synced inode relative to the held directory without pathname temporary races.
Do not overwrite existing scripts or bundle metadata. Unsupported platforms and
filesystems fail closed; later ambiguous failure remains an unknown outcome.

Retain per-axis request IDs before dispatch in the private client journal. Each
axis can gain completion only from its original positive Engine acknowledgement.
Known completed channels survive later failure; missing positive proof cannot be
replaced by matching file contents or a new ID. The result is per-file atomic and
recoverable, not an aggregate atomic merge or fresh contents/quality proof.

See [contract](../../docs/PULSAR_LIBRARY_PARTIAL_BUNDLES.md) and
[evidence](../../docs/evidence/roadmap-2026-10-12/library-partial-bundles/checkpoint.json).
