# ADR B3-205: retire confirmed recording handoffs without deleting history

Status: implemented with scoped native and mock-player evidence. 2026-10-13.

The durable recording-stage journal allowed only one original candidate intent. After
its confirmed commit, a second take on the same project could not prepare a different
stage request without discarding metadata manually. Recoverable iterative authoring
requires a deliberate terminal transition.

Extend the existing journal with optional retirement/history fields. Preserve legacy
reads and identity. Retire only a confirmed stage+commit, with explicit acknowledgement
and exact identity/revision bindings. Verify the candidate through current Engine read
and check committed project progress before changing metadata. Retain compact receipts;
keep all Engine artifacts and recordings. A subsequent preparation keeps the history
while replacing only the inactive handoff. Late old retirement replays cannot clear a
new active intent. Enforce 64 unique historical stage/commit identities and the existing
private journal byte bounds; reject unknown commits and malformed/foreign history.

These receipts locate original history; they carry no new review, approval or authority.
Gap proposals and recording-index reclamation remain independent open lifecycles.
See [contract](../../docs/PULSAR_BROWSER_RECORDING_CANDIDATE_RETIREMENT.md).
