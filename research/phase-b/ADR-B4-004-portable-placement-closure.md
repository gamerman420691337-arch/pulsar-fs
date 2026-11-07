# ADR B4-004: Versioned portable placement closure

Status: implemented locally; broader platform, media and release qualification remains open.

## Decision

Portable format 3 explicitly carries every retained immutable placement definition, active membership, revision membership and retained edit-history membership. It keeps missing legacy membership rows absent. Definitions are sorted by ID; membership is bounded to 128 unique ordered IDs, definitions to 4096, and all nested membership entries count toward the existing manifest admission budget. Every definition requires its retained media source and creation revision. Active membership must agree with captured head and history cursor.

Authorized stored-package reopening now checks the exact receipt format version in addition to existing header/size/manifest checks; changed known versions are refused, as are unknown versions. This fast reopen remains distinct from full byte-digest verification.

Formats 1 and 2 retain their canonical serialization: optional placement fields and zero counters are omitted. Format 3 requires a nonempty definition closure even when all placements have been removed from active membership. Existing package commands, framing, materialization, descriptor/object verification and authorization remain the transport and publication boundary. No source locator, credential or executable worker request is introduced.

Clone import assigns a complete bijective fresh placement namespace alongside project/candidate identities. Source versions remain their verified content identities, and creation revision ordinals and alignment specifications remain unchanged. Publication installs definitions and membership in the same authenticated transaction as the cloned project, after required source objects exist. Undo/redo therefore restore imported placement membership using the engine's ordinary edit path.

Explicit origin maps retain local-to-original placement identities. Origin validation verifies definition alignment, mapped revision membership, immutable initial placement history, complete original coverage and transitive namespace composition. Equal mapping payloads do not authorize changed identity ancestry. Later local alignment edits add their own immutable definitions and revisions without rewriting archived origin bytes.

## Evidence boundary

A pure clone case exercises successive namespace remapping, altered-anchor rejection, omitted ancestry, incomplete fresh assignments, wrong format, inconsistent active membership, wrong source kind and invalid history coordinates. An authenticated process case starts with protected authored motion, replaces and undoes alignment, exports/imports, restores inactive definitions with redo, restarts, removes active membership, and clones/re-exports a second time. Neutral motion stays unchanged.

The retained schema-2 executable is separately exercised on a format-3 import declaration: refusal must occur before any project publication. This exact binary is the compatibility subject, not every historical reader. Schema-3 database refusal by that binary is separately retained in the prior placement checkpoint.

This closes the local placement-package implementation gap. Placed candidate generation is implemented separately in [ADR B4-005](ADR-B4-005-placed-generation-sampling.md). This does not qualify explicit motion retiming, arbitrary codecs/platforms, real-media quality, GUI usability, audio synchronization or physical playback. GB-03 and the full roadmap remain open.
