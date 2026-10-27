# ADR B3-204: preserve explicitly approved recording commit requests

Status: implemented with scoped native reply-loss/server and Engine-restart evidence. 2026-10-13.

Recording-stage recovery formerly restored selection only. Its later commit approval
and request lived in memory, so adapter death after a lost commit reply could not
reconcile the original approved operation.

Extend the private recording-candidate journal with a backwards-compatible optional
commit intent. Save it only after fresh diagnostics, explicit acknowledgement and
current candidate/project preflight. Bind candidate, revision, digest and next-revision
acknowledgement. Recover this original request without restoring a reviewed pin. Never
invent approval for legacy journals or unapproved candidates.

Explicit retry rechecks the candidate through current Engine authority, dispatches the
original request and checkpoints its acknowledgement under the workflow lock. The
Engine's authenticated receipt/revision semantics remain authoritative. In-memory
refresh must not override a durable acknowledgement with an older local copy.
Recording and gap recovery remain separate selections; this slice deletes no data and
performs no device action. Journal retirement and broader fault/platform acceptance are
separate remaining work. See [contract](../../docs/PULSAR_BROWSER_RECORDING_COMMIT_RECOVERY.md).
