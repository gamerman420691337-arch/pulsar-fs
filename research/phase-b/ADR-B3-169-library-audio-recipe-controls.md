# ADR B3-169: shared desktop audio recipe authoring

Date: 2026-10-12. Status: implemented scoped software; full roadmap PARTIAL.

## Decision

Reuse the existing single-file audio drafts and controls for direct six-axis retained batch intake. Keep recipe JSON as an explicit alternative. Snapshot checked fixed-axis settings at capture; perform folder discovery in the existing background controller; retain full Prepare options and original request identity after possible dispatch. Do not write an intermediate recipe file, recapture edited settings on retry, or add Engine/Protocol/storage contracts.

Canonicalize imported channel order into Axis::ALL while preserving per-axis settings. Share recipe snapshot and presentation code with single-file Audio generation. Local discovery failures do not dispatch and leave the editor usable; unknown acknowledgements retain existing exact recovery policy.

## Evidence and boundaries

See [contract](../../docs/PULSAR_LIBRARY_RECIPE_AUTHORING.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-12/library-recipe-controls/checkpoint.json). Four new controls/import/recovery tests and both actual native editor/file workflows pass. All 24 script outputs match retained authored timing oracles. Client tests pass 370 with ten ignored; both strict lint configurations and remaining affected checks pass. Seven native Engines are joined. The initial ordered-array fixture mismatch remains recorded; per-axis checks and reordered-import regression now pass.

No rendered GUI, useful creative/model default, independent real-media, platform, physical Handy or human/release acceptance is established. Visual/model retained folder workflows and other roadmap obligations remain open.
