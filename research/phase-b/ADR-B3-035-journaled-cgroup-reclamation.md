# ADR B3-035: retain retirement identity across directory deletion

Status: implementation connected; native, reader and Rust gates pass; 104 configured JavaScript cases pass without skips.

Clearing an active roster before removing its directory loses the identity needed to reclaim an orphan. Removing the directory before clearing an active record leaves startup unable to distinguish a safely retired group from a missing unresolved group. Use a durable retirement tombstone between those operations.

Host-local fence format 2 carries separate active and retired maps. Preserve canonical format-1 scope/lease identity during migration, validate the entire roster before cleanup, and refuse a missing active record. A tombstone permits retry after acknowledged directory removal; existing entries still require exact identity and emptiness. Never scan or signal unrelated groups. Observe peak memory before removing kernel files, and retain it only after successful reclamation.

The database reader barrier stays 17 and portable format stays 9. Retain a source-bound format-1 reader and verify positive old-state opening followed by refusal of format 2 without committed DB/WAL mutation. Do not describe seeded native journal cuts as actual crashes or power loss. Unregistered creation windows, unrecorded orphans, failed staging cleanup, hostile concurrent same-UID path replacement, independent host IO deadlines and the remaining GPU/device/platform/release obligations are not closed by this decision.

See the [contract](../../docs/PULSAR_CGROUP_RECLAMATION.md).
