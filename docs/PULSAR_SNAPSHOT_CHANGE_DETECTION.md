# Source changes with restored modification time

Snapshot capture and worker reuse now compare Unix change timestamps, including nanoseconds, in addition to length and successfully retrieved modification timestamps. Fresh descriptor capture, worker-source hashing, existing-object verification and the final source recheck use the same predicate. Unavailable modification metadata refuses rather than treating two lookup failures as unchanged.

A real authored file is rewritten from `first` to `other`, preserving length, and its modification time is explicitly restored. The fixture requires an observable different change timestamp before counting the assertion. The preceding production predicate accepts this changed file: the new regression fails. The final predicate rejects it; the unchanged control still passes. This verifies metadata comparison through a real filesystem operation, not an injected capture-time race campaign.

The Linux [inode contract](https://man7.org/linux/man-pages/man7/inode.7.html) distinguishes modification time from status-change time. Read access time is excluded from the predicate. Timestamp granularity and filesystem semantics still limit observable change detection. The content hash remains the identity of captured bytes. This does not guarantee an atomic source view on every filesystem, protect against privileged modification or replace anchored private storage and publication rules.

Windows private snapshot publication remains unsupported. This increment supplies no Windows secure-storage backend, native platform acceptance, model accuracy, real-media/device evidence or roadmap completion.

Evidence: [checkpoint](evidence/roadmap-2026-10-12/snapshot-change-metadata/checkpoint.json). Architecture: [ADR B3-149](../research/phase-b/ADR-B3-149-observable-source-change-metadata.md).

Final serial Engine verification passes **842 tests, 0 failed, 70 ignored**. The initial parallel run records **841 passed, 1 failed, 70 ignored**, with an unrelated decoder fixture reporting `Text file busy (os error 26)`; no root cause is established. Both strict Linux lint configurations, Windows GNU workspace/all-target source check, native Linux App/Worker build, formatting, 445 architecture checks and 36 source guards pass. Clients/Core/Protocol and Wine execution are not rerun in this increment. No all-feature runtime or native Windows qualification follows from these checks.

A separate [native copy-fallback campaign](PULSAR_SNAPSHOT_CAPTURE_RACE.md) now exercises a real restored-mtime rewrite after the source metadata read. The preceding predicate publishes changed bytes; the final production helper refuses, removes its temporary member and preserves an unrelated marker. Its distinct controls, injected subjects and source/executable identities are retained separately from this original predicate checkpoint.
