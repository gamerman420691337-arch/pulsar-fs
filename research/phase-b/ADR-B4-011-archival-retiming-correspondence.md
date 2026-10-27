# ADR B4-011: Inspect clone retiming through a separate historical contract

Status: implemented engine/CLI/desktop archival lookup; executable cross-namespace proof open.

Format 5 retains original retiming facts, but copying them into destination-local receipt tables would fabricate session/request/actor authentication. A separate scoped Read command therefore returns strict historical facts and their verified destination correspondence. Local receipt lookup retains its existing meaning.

The reader admits memory, bounds SQLite cells before allocation, verifies the immutable identity map and exact canonical origin manifests through pinned no-symlink evidence reads, and checks complete semantic mappings and composition without inventing a flat-container identity for the retained clone projection. Exactly one original native receipt must map to the selected local revision. Local attribution, parent/edit kind, retained placements, historical target membership and motion identities must match; recomputed retiming must equal the verified historical result.

The response keeps the original receipt unchanged and marks attribution historical-only. It transfers no original session, request, grant, playback or qualification authority. Current local motion remains unchanged. Client and desktop checks prevent another project's result or an obsolete selected revision from being presented as current.

Aggregate import accounting additionally charges placement/retiming records and placement maps; pure graph validation charges root logical records. Protocol refusal tests and compiled successive-clone, authority, corruption, symlink, oversized-cell, attribution and restart cases accompany [the contract](../../docs/PULSAR_ARCHIVAL_RETIMING_STATUS.md). Full ancestor container/source-byte reconstruction remains the import gate. Portable Stash association migration and executable clock proofs remain open.
