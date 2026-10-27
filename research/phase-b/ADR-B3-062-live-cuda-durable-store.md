# ADR B3-062: Conditional schema21 for original live CUDA evidence

Status: Implemented private store and startup validation; verification in the linked checkpoint. Public finalization remains pending.

Retain the immutable admitted object's four blobs in the project database rather than publishing separate files with orphan-cleanup races. Add a conditional schema21 barrier in the same transaction as its exact table and first row. Preserve rollback, row capacity and existing format-4 ownership requirements. Expose schema20's unchanged layouts to existing readers/writers only after checking the actual schema21 header and new table. Refuse mismatched, downgraded and future state. Do not upgrade ordinary CPU databases merely because new software opens them.

Validate canonical controls and original native bytes through the bounded admitted reader before ledger recovery and again before writable WAL startup. Require exact indexed identities, source membership, and historical revision ordering. Do not resolve any stored guest path or infer launch authority from parsed receipt declarations. Reserve before Rust blob loading, retain admission through readback/publication, and reserve storage from actual free space before entering the transaction.

The private writer checks current project revision and storage admission; the future Engine finalizer must independently consume its owned live worker and recheck grants, lease, fence, registry membership, attempt and closing state. The native storage fixture deliberately publishes after explicit Stop, so it cannot establish that missing finalizer contract. Preserve that distinction in reports and qualification claims.

See [implementation, verification and limits](../../docs/PULSAR_LIVE_CUDA_DURABLE_STORE.md). No model/media, Handy, platform, six-axis latency, human or release gate changes.
