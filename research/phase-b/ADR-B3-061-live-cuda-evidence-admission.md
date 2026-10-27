# ADR B3-061: Retain final live CUDA bytes under memory admission

Status: Implemented; verification in the linked checkpoint. Durable publication remains open.

Reserve final-evidence memory before private consuming Finish receives native output. Transfer the reservation into an immutable evidence object and release it only when the object is dropped. Use the caller's shared Engine resource pool and a memory-only reservation, preserving interactive worker capacity. Child/cgroup, GPU and capture custody retire independently at Finish under the existing original deadline. Refusal must drop admission and retire the consumed worker through existing custody handling.

Keep canonical manifest/summary control bytes and original receipt/trace bytes with distinct serialization rules. Bound every blob before parse, revalidate all existing strict live-result correspondence, and prevent mutation of validated fields. The planned SQLite writer must reserve before loading blobs, validate historical project membership/revision and require an exact durable reader barrier before publication. Consistent bytes supplied by a caller do not establish Engine ownership, current grants, revision, lease or launch provenance.

Keep receipt consistency independent of the native SDK worker type. This permits default-feature readback without loading CUDA and provides the pure validator needed for durable recovery. It does not qualify Windows/macOS path semantics or runtime behavior. The 512 MiB reservation is cooperative accounting for JSON expansion and retained originals, not measured peak memory or OS enforcement.

See [implementation, tests and limits](../../docs/PULSAR_LIVE_CUDA_EVIDENCE_ADMISSION.md). Public GPU routing stays disabled while finalization/persistence and scheduling are incomplete. Independent product and release requirements remain unchanged.
