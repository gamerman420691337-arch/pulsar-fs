# ADR B3-202: retain browser recording identity across adapter loss

Status: implemented with scoped native recovery evidence. 2026-10-13.

The browser previously retained recording identities only in Preview memory.
Adapter death therefore lost the selection needed to read samples still held by
the surviving Engine. This obstructs recoverable live authoring.

Retain a private session/project index before the original Begin dispatch. Store
only original identities and revision, not inferred samples or authority. Reuse
the existing locked private recovery transaction, schema validation and byte bound.
Each append reads the current index under its lock; startup/refresh read without
mutating it. Reject duplicates and more than 64 scopes. Publish the launch URL only
after recovery validation succeeds.

Recovery is metadata discovery. Engine readback remains authoritative for active,
stopped, discarded or unavailable state. Never replay a Begin automatically or
invent an empty recording after boot loss. Candidate assembly, review, commitment
and export remain separate explicit operations. The retained bound intentionally
survives adapter restart; explicit retirement is future work.

See [contract](../../docs/PULSAR_BROWSER_RECORDING_INDEX.md). Model, device,
platform and release qualification are unaffected.
