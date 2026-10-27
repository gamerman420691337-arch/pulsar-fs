# ADR B3-206: retire confirmed gap handoffs for iterative browser authoring

Status: implemented with scoped native two-cycle and mock-player evidence. 2026-10-13.

A completed durable gap proposal continued to occupy both local selection and its
private journal. It blocked another recording stage or repair on the same project,
even after recording/export handoff retirement.

Add backwards-compatible optional retirement/history fields to the gap journal.
Share a compact RetiredProposal receipt with recording retirement while preserving
independent namespace/selection. Retire only an exactly identified, acknowledged
commit after explicit consent and current authenticated candidate/program/project
readback. Recover no active gap from retired metadata; stage and commit refuse it.
Preserve compact history on subsequent preparation and make late old retirement
idempotent without changing newer active work. Bound history to 64 unique identities
and keep the existing private storage byte/ownership rules.

Do not delete Engine artifacts or transfer receipts and do not reconstruct recording
buffers. Preserve synthesized evidence and independent quality/device/release gates.
Recording-index capacity and history reclamation remain open. See
[contract](../../docs/PULSAR_BROWSER_GAP_RETIREMENT.md).
