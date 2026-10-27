# ADR B3-167: durable local batch cancellation and partial metadata release

Date: 2026-10-12. Status: implemented software contract; scoped verification. Full roadmap PARTIAL.

## Decision

Add explicit owner-scoped `library-batch-cancel BATCH --request-id CANCEL_ID`. Store stop policy and claim the exact outer request before effects, then retain ordinary CancelJob identities for known Generating jobs. Preserve terminal jobs, completed projects and published outputs. Cache the outer cancellation receipt atomically with its completed record only after all known acknowledgements are resolved. An incomplete cancellation retains its original identity and error for exact retries; a different ID cannot take over.

Reconcile a pending generation phase only from its exact positive cached acknowledgement. Never redispatch an original unresolved create/import/generate/commit operation while stopping. Retain unknown/conflicting receipts and pending identities. Keep bearer fingerprints authoritative rather than silently migrating authority after credential rotation.

Raise the outer database reader floor to 29 before cancellation effects, preserving the existing underlying layouts and inner schema writers. Reject cancelled records under a downgraded header. Reports without cancellation keep schema 1 and their previous serialization; cancelled reports use schema 2. Current generation/publication clients observe stop policy before bounded decisions, preserve their existing journal schema/receipts and distinguish cancelled workflow state from completion. In-flight publication has no rollback guarantee.

Permit explicit metadata release for completed cancelled partial batches only when retained jobs are currently terminal and absent from the Engine worker map. Keep source/media/output files and all downstream project/job/candidate/export records. Unknown acknowledgements, active handles and other batch memberships remain protected.

## Boundaries and evidence

Cancellation completion is durable stop policy plus ordinary known-job acknowledgement, not verified physical worker retirement. worker_retirement_verified and qualified remain false. See [contract](../../docs/PULSAR_LOCAL_BATCH_CANCELLATION.md) and the [evidence packet](../../docs/evidence/roadmap-2026-10-12/library-cancellation/). General migration, unknown-receipt disposition, continuous watching/intake, collection, broader fault/GUI/platform/model/media/device/human and release qualification remain open.

The selected-package suite passes 1,522 tests (80 ignored); final clients pass 359 library/integration tests (ten ignored) after the unused-assignment repair. Both strict lint configurations and remaining retained checks pass. Native cancellation/restart/replay/partial release/failed-job preservation and the preceding scheduler regression pass. The older reader refuses without main database/WAL change, but changes SQLite shared-memory bytes; the sidecar preservation assertion remains FAIL. Nineteen Engine instances are joined; no native in-flight retirement or physical-device claim follows.
