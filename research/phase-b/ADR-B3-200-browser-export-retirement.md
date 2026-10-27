# ADR B3-200: Retire confirmed pointers while retaining original export facts

Require explicit preserve-files acknowledgement bound to original request, revision
and confirmed byte digest. Move only a completed active reference into compact
retained history; preserve scripts, side journals and Engine receipts. Replay old
retirement without clearing a newer active intent. Read history through validated
pinned storage and authenticated original Engine request replay.

Accept older single/null records non-destructively. Bound history and metadata;
refuse uncertain/changed/full cases without losing intent. Actual file reclamation,
legacy orphan reconciliation and broad crash/platform qualification remain open.

See [contract and evidence](../../docs/PULSAR_BROWSER_EXPORT_RETIREMENT.md).
