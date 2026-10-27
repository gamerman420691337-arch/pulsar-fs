# ADR B3-203: original browser recording-stage recovery

Status: implemented with scoped native reply-loss/restart evidence. 2026-10-13.

A recovered recording list alone cannot reconcile a staging reply lost before
adapter death: Preview formerly forgot the original request and options. Creating
another stage request would duplicate candidates and could fail after boot loss.

Persist the original stage identity and options at preparation, checkpoint its
attempt before dispatch, and retain its confirmed candidate ID afterward under
a private workflow lock. Recovery reads inert selection only; explicit replay uses
the original Engine request. Preserve one retained intent, reject changed preparation,
and validate candidate/project/revision/job binding before acknowledgement.

The journal is metadata, never native recording authority or a sample backup.
Current Engine authentication and durable receipts remain decisive. Do not persist
reviewed pins or commit consent in this staging slice. Same-process refresh can keep
its existing reviewed/approved in-memory state only for the same request and candidate;
cold restoration clears both. Recording commit recovery remains separately open.

See [contract](../../docs/PULSAR_BROWSER_RECORDING_CANDIDATE_RECOVERY.md).
