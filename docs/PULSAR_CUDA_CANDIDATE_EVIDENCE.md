# Durable CUDA candidate evidence

The engine job path now preserves `LaunchExecution` through completion instead of discarding its optional CUDA evidence. Successful CUDA generation requires the parent's checked dedicated receipt and original trace; CPU completion refuses attached CUDA evidence. This does not enable production GPU admission or launch.

## Completion and membership

Completion independently rehashes the parent-owned CUDA files and checks their request/profile/model/runtime/owner-policy/device/run bindings through the existing verifier. It validates the motion program and general receipt, then includes the dedicated receipt and original trace identities in candidate lineage.

Before publishing a CUDA candidate, the transaction requires the exact canonical request in both the current running job and running attempt, with immutable intent belonging to the current engine epoch. Existing Generate authorization and late-attempt state checks still apply. Missing or changed evidence, an unexpected provider, or another engine epoch fails without a candidate.

The candidate, completed job and attempt, `cuda_candidate_evidence` row and conditional schema-16 barrier publish in one transaction. The row binds candidate and unique intent attempt, canonical request digest and canonical typed receipt/trace references. The retention helper uses a savepoint so a failed call cannot leave its layout or barrier change behind. It cannot replace an existing membership. Persisting this row does not grant Generate permission, prove admission or attest native provenance; its caller remains responsible for those conditions.

The two artifact roles have fixed parent filenames. Metadata is limited to 32 KiB per record, 4,096 rows and 64 MiB total. Scalar type and byte bounds precede deserialization. Candidate scope/revision/job, both lineage identities, immutable request digest, completed attempt and current job membership must agree. The referenced receipt and trace bytes are independently checked again.

## Reader behavior

Read-only startup validates accepted membership before writable SQLite/WAL initialization. Schema 16 requires the exact evidence layout and every completed CUDA attempt to have membership. Older versions refuse nonempty accepted evidence or completed CUDA candidates without the barrier. Missing rows, altered metadata, missing lineage, mismatched job state and changed artifact bytes fail closed.

Intent validation and placement migration now preserve schema 16. CPU-only projects retain their existing version; CUDA intent alone still uses schema 15. Completed accepted evidence survives a new engine epoch, while incoming completion must match the live epoch. Old schema-15 readers refuse before writable startup.

Portable project archives do not yet carry this role mapping and its original artifacts. Their common capture path explicitly refuses affected projects instead of omitting the accepted CUDA evidence. CPU project archives remain supported. Portable accepted-CUDA closure is required before production GPU scheduling is enabled.

## Evidence scope

The actual completion-transaction tests use authored consistency receipts and traces. They establish atomic membership, lineage, epoch/provider refusal and CPU barrier preservation, not genuine native provenance or launch authorization. The separate native database fixture likewise authors its evidence, then exercises real startup, restart, schema preservation, and eleven old/corrupted-reader refusals against committed nonempty WAL files. The main and WAL bytes remain unchanged on refusal.

Rebuilt confined CUDA generation continues through the existing real worker and parent artifact pipe in a separate scenario. It is not an engine-admitted GPU job. These scopes must not be combined into an end-to-end GPU acceptance claim.

The initial native fixture failure collided a corruption case named `state` with its live state directory. Its source subject and log are retained separately; the corrected test uses distinct directory names. The unknown-schema retiming fixture advances from 16 to 17 because 16 now has defined semantics.

The [source-bound packet](evidence/roadmap-2026-10-01/cuda-candidate-evidence/validation.json) records 908 default Rust passes, 447 CUDA-feature engine passes, all 102 configured JavaScript cases without skips, 225 structural checks and 46 unchanged executable/metadata identities. Its 175 production and 126 verification paths have archive and current-source readback. Seven default qualification cases remain ignored. All 69 retained artifact hashes have readback.

Independent parent IO deadlines, native trace authenticity/completeness, physical VRAM enforcement, engine-owned GPU admission and launch, confirmed retirement, cancellation/restart custody, portable CUDA closure and model/media/Handy/platform/human/release qualification remain open. Publication failures can still leave a running job until recovery if the completion transaction cannot commit; full lifecycle integration must address that case.
