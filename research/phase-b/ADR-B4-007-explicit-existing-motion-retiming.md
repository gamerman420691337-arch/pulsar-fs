# ADR B4-007: Explicit motion retiming uses retained placement clocks

Status: implemented exact covered-support edit; broader trim/proof/UI work open.

[B4-003](ADR-B4-003-engine-owned-source-placements.md) deliberately changes placement membership without changing motion. The specification and GB-03 still require existing-motion editing. Reusing the placement-change operation to retime implicitly would break that behavior and mix source alignment with authored motion changes.

A separate authenticated `RetimeMotion` operation names the retained old placement, active new placement and explicit axes. The engine checks Edit authority/current revision, looks up both definitions in the same project, invokes the pure exact-rational clock transform and uses the existing protected-segment/revision/history transaction. It preserves position/evidence values and other tracks. Source/trim mismatches, negative output, overflow or quantization collisions refuse atomically; destructive trim editing does not silently discard support.

Actions floor to nanosecond project timestamps. Unknown gaps round outward, with conservative reverse endpoint coverage, and cannot collide with actions. This chooses uncertainty retention over silently resolving unavailable intervals. Some boundary layouts refuse because the current half-open nanosecond representation cannot preserve them safely. Exact source/project definitions and immutable before/after motion remain retained; this is not an actual decoded-frame or physical timing measurement.

The edit records `source_retime` lineage, preserves original review flags, retires their decisions and adds a review-required marker with unknown/conservative provenance. Stash refuses this operation in source-clock derivation: an earlier candidate's original sample plan cannot be applied to retimed action times. Implementing retiming-aware export proof is a subsequent requirement, not a reason to claim correspondence from an old receipt. Existing portable lineage carries this declaration without granting execution authority; no project schema or package-format shape changes.

Tests independently check expected forward/reverse timestamps and unchanged values, fractional quantization/collisions, unknown gaps, selection/protection, owner/revision gates, replay and restoration. Compiled native CLI fixtures use authored script values and an inert source that is not decoded. Real source/model correspondence, desktop presentation, destructive trim boundaries, proof migration, projected formats/platforms and device synchronization remain open.
