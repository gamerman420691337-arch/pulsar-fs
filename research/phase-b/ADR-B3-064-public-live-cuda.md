# ADR B3-064: Own public CUDA capture scheduling and preserve live provenance

Status: Implemented and locally verified. Broader qualification remains pending.

Route explicitly declared CUDA capture profiles through an Engine-owned scheduler. Keep the ordinary persistent worker CPU-only. Mint the opaque runtime scope in the existing authenticated registry, inherit its original lease through startup and actual CUDA custody, and retain the actual worker in one thread. Expose asynchronous Finish with a single `FinishRequested` phase: persist its acknowledgement before freezing intake/dispatch/continuation, drain an issued pair without provisional publication, then consume the worker through the existing finalizer. Replays observe current state and never rearm it.

Do not manufacture imported-source rows to satisfy schema21 evidence foreign keys. Retain public capture evidence in a separate conditional schema22 table with canonical origin provenance linked to the initial request receipt. Keep the prior table/layout intact and validate both before exposing the unchanged schema20 base to existing layouts. Share bounded row/aggregate capacity and account for origin memory/storage before reading/publishing. Historical metadata cannot supply current grants, lease, launch or device authority.

Preserve original native receipt/trace bytes and CPU report bytes. Reject new database state with a preceding compiled reader before WAL/ledger changes; the current reader must reopen it. Verify public Engine admission/scheduling/Finish directly, without test registry insertion, and distinguish those checks from actual CLI/socket, real-media and device qualification.

See [implementation, verification and limits](../../docs/PULSAR_PUBLIC_LIVE_CUDA.md). No learned-model, Handy, platform, six-axis latency, human or release acceptance changes.
