# ADR B4-009: Preserve exact source clocks through verified retiming chains

Status: implemented for supported single-clock histories; portable and mixed-history closure open.

Original generation samples have exact rational source clocks, while authored retiming operates on quantized project timestamps. Applying the origin sample plan to the final timestamps or merely inverting the final placement would silently invent correspondence or lose fractional sample precision.

The engine therefore verifies each structured retiming receipt, recomputes its complete recorded edit and follows bounded effective stroke lineage. A pure mapper carries the origin exact clocks through each actual historical before-track using the same quantized placement mapping as the edit. Authored timestamps use the declared inverse and carry no generation-frame claim. Updates are atomic; collisions, trim loss and discontinuous placement specifications refuse. Memory admission covers carrier/output lifetime and each historical read.

Mixed stroke merges and rebased generation with differing clock histories refuse until an explicit piecewise correspondence proof exists. Undo/redo follows restored state. Stroke publication requires explicit source-time intent, preserves the master and stores ordered verified retiming facts in a schema-2 private proof. Existing schema-1 proofs remain readable. This changes neither project database schema 6 nor portable format 4, and creates no external identity, perception, device or release authority.

Pure fractional-clock, authored-time, atomic-refusal and lineage composition tests supplement native compiled CLI/mock-Stash publication, corrupt-receipt refusal, exact source timestamps, unchanged master, replay, proof observation and restart. See [the contract](../../docs/PULSAR_RETIMED_STASH_PUBLICATION.md) and its exact validation receipt. Portable receipt closure, destructive trimming and actual media/device qualification remain required work.
