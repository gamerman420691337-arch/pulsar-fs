# ADR B6-006: Transactional protection of native authored motion

Status: implemented locally; qualification remains open.

A committed native values-only edit must survive ordinary regeneration by default. Candidate staging already retains an engine-computed authored footprint, but previously committing it left project protection unchanged. A later regeneration could replace the user's edit.

Compute changed motion from the actual authoritative pre-commit program and actual committed program, then union this footprint into the bounded project policy in the same transaction as revision, history and lineage. Preserve existing global/axis protection; coalesce same-axis overlaps/adjacency; refuse overflow without partial mutation. Imported authored declarations remain archival and cannot trigger native authority. Merge contribution attribution uses the parent revision's policy rather than the new automatic locks.

Explicit policy replacement is revision-bound and ManageProtection-scoped. Exact native undo/redo can restore changed protected motion only with fresh ManageProtection. Ordinary replacement stays forbidden until explicitly unlocked. Existing-project regeneration uses the existing merge kernel over candidate axes, retaining missing axes, protected stencils and review warnings.

No retroactive history inference is performed. Legacy edited projects need explicit policy review; rendered UI, natural-media/model quality, Handy and platform/human/release qualification remain separate gates. See ../../docs/PULSAR_MANUAL_EDIT_PROTECTION.md for the interface and evidence boundary.
