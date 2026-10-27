# ADR B3-103: runtime installation stage recovery

Status: implemented; source-bound default library/lint/native verification PASS. Scope: explicit local owner inspection/discard of recognized inactive Linux installation stages.

Problem: process death can leave an unpublished installation stage. Manual recursive deletion cannot distinguish an active writer, a known partial candidate and unknown user files.

Decision: before copying payloads, create a private single-link stage receipt containing the original bounded manifest JSON and its SHA-256. Hold an exclusive file lock throughout installation and synchronization/publication. The receipt remains as private local metadata in the published installation; it is not a manifest payload or a model/inference approval. The published basename is not eligible for stage discard.

Expose runtime-stage-inspect and runtime-stage-discard through the trusted executable host before Engine configuration. Require an absolute no-follow stage path with the generated basename convention, an owned parent not writable by other users, a private owned stage and private regular single-link bounded receipt. Inspection reports Locked without treating unfinished receipt bytes as an identity. An unlocked receipt must validate its original manifest identity, schema and finite file membership before inventory is reported.

Discard requires explicit approval and the exact inspected manifest hash. Acquire the receipt's exclusive lock nonblocking; preflight every finite directory/file entry before any mutation. Refuse unknown names, symlinks, hardlinks, unexpected ownership/modes/devices, oversized contents, corrupt receipts and stale identity. Remove only known entries using held descriptors with device/inode binding checks, keep the receipt until last and synchronize removals. A failure can leave an explicitly reported partial discard; retry rechecks the surviving receipt/inventory. An uncertain final parent sync is surfaced rather than returned as success.

Pulsar locks coordinate its own writers/discard operations; arbitrary software with the same user's full OS privileges is outside this boundary. Legacy unmarked, partially written or corrupt receipt stages remain unrecognized and are preserved for manual inspection. No automatic garbage collection, update/rollback, signed release, complete filesystem fault/power-loss matrix or Core/Complete delivery is established.
