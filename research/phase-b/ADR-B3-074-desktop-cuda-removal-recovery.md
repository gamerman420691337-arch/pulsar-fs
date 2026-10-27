# B3-074: Durable desktop removal recovery

Status: implemented; scoped software, owned-process-death and selected native reopen/retry checks passed.

Persist an exact, non-authoritative removal intent before network dispatch. Restore entries through the background evidence worker without automatic effects. Bind the journal to the session, credential digest and endpoint; retries retain the original project/revision/epoch/manifest/request ID and still cross current Engine authentication and authorization.

Retain validated acknowledgements separately from pending entries. Only acknowledged entries may be dismissed; clearing an in-memory selection does not delete unknown history. Bound capacity and fail closed before new dispatch rather than evicting unresolved requests. Keep the Engine's existing durable command journal as authority.

Extract pack recovery's private filesystem transaction into a shared storage module without changing its namespace, JSON schema or validation. Removal uses a distinct journal and validates full receipt correspondence before acknowledging. Unsupported platforms and corrupt or mismatched private storage refuse dispatch.

See [behavior and limits](../../docs/PULSAR_DESKTOP_CUDA_REMOVAL_RECOVERY.md) and the [evidence packet](../../docs/evidence/roadmap-2026-10-08/desktop-cuda-removal-recovery/). Actual window/human QA, physical power-loss durability, broader crash cuts and product/release qualification remain open.
