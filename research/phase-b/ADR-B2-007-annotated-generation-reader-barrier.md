# ADR B2-007: conditional annotated generation reader barrier

Status: implemented locally; public annotated generation remains guarded.

## Context

The annotated worker retains a new source-bound preprocessing contract and schema-3 lineage. Persisting those requests under an older reader header could permit recovery by software that does not enforce that contract. Existing live CUDA evidence and captures already use conditional schema-21/22 storage and must remain readable without rewriting their original bytes.

## Decision

Use schema 23 only when annotated membership is retained. Add an exact marker singleton and an immutable attempt/job/request-digest index. Infer the preserved underlying 20/21/22 layer from its tables, then run its existing validators. CUDA writers may add their existing tables but preserve header 23. Refuse missing markers, unknown versions, downgrade attempts, incomplete membership and changed immutable requests.

Validate bounded metadata read-only before custody, WAL mutation or recovery. Bind each request to its explicit CPU annotated profile, dependencies, immutable source, project and job/attempt history. Do not open historical worker paths. Use bounded serialization, bounded retained count/bytes and admitted payload memory. Publish membership inside the job transaction and a rollback savepoint.

## Evidence and consequences

[Storage evidence](../../docs/PULSAR_ANNOTATED_GENERATION_STORAGE.md) records authored reopen/refusal, retry, rollback and CUDA coexistence tests. The preceding executable header probe establishes only version refusal on an isolated header fixture. It does not qualify a full historical project migration.

The implementation changes persistence capability without enabling public annotated generation or migrating user databases. Strict receipt correspondence, portable closure and client/public-path tests remain prerequisites to removing guards. Capacity exhaustion refuses; disposal policy, broader recovery/power-loss and release qualification remain open.
