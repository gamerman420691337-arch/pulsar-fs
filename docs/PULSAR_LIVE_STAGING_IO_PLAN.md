# B3: bounded live snapshot preparation

Status: verified counterexample and successful released-read control; admitted preparation implementation present; workspace build and three controlled native cases passed; all 108 application cases passed without skips; the declared Rust/native/feature and structural gates passed. Cleanup-failure and restart qualification remain open. The architecture-first roadmap and software specification remain authoritative.

## Stage card

- **Goal:** keep persistent live preparation responsive to the original cancellation/lease deadline while filesystem capture is blocked.
- **Claim:** the caller can retire preparation without publishing a late bundle or releasing resources still owned by preparation. The controlled native read must no longer hold that caller past its budget. This does not establish a universal hard real-time host-I/O guarantee.
- **Deliverable:** a preparation module with explicit worker/model/runtime roles, independently supervised capture, opaque prepared ownership and integration into `LiveInferenceWorker::start_owned`.
- **Gate:** the held-read native case returns on the original deadline while the read remains held; the released-read control still produces three estimates and retires its model process. Cancellation, late completion, retained admission, source replacement, changed bytes, cleanup failure and restart ownership need separate cases.
- **Means:** inspect and reuse confinement, pre-spawn custody, durable worker fencing and confirmed retirement. Capture/hash/sync work must leave the caller's blocking path. Preserve selected descriptor identity and namespace authority; source resolution must not widen allowed reads. Introduce an explicit typed capture request if the current native-library request cannot express worker/model/runtime roles faithfully.
- **Provenance:** the preceding 308-path source subject and separate implemented 310-path source subject, compiled native identities, debug filesystem source, two RAM-backed failures, same-filesystem held/released controls and full application failures are retained under `live-staging-io` and `runner-reflink-capture`.
- **Rollback:** preserve the preceding source and failed cases. Refuse live preparation when supervision is unavailable; do not silently resume blocking capture or enable another provider.

## Implementation order

1. Specify the preparation ownership transition before editing production: admitted → capturing → prepared or retiring. RAM, worker-slot, storage, directory and descriptor custody must have one owner at every transition. Preserve an absolute deadline from the original lease; a helper's launch must not reset it.
2. Add the capture boundary and strict bounded correspondence checks. No model/runtime loading or inference occurs during capture. Bind original admitted identities, exact roles and request identity; parent publication must not accept child-selected host paths or rehash large data synchronously.
3. Connect preparation to common pre-spawn custody and retirement. Unconfirmed termination or cleanup keeps its claims and directory ownership. A helper cannot outlive its accounted capacity or permit the inference phase to overlap beyond its admitted bounds. Preserve required reader/restart barriers if durable ownership changes.
4. Replace the synchronous snapshot loop in `live_inference.rs`. Recheck cancellation, lease, current authority and exact captured identities before executable preparation or inference launch. Refuse late completion without project motion or device effects.
5. Run the controlled held/released cases against rebuilt native binaries, then the affected live/CLI cases and complete current-source application campaign. Run appropriate Rust default/native/feature gates if their source changes. Archive current source and all executable identities; preserve every initial failure.

## Remaining boundaries

Metadata/path selection, publication, cleanup, engine startup and other host-I/O calls require their own analysis; a read-only model-file regression cannot prove all of them bounded. Do not replace durable ownership with detached work whose resources are forgotten. Creation journaling, orphan recovery, GPU/native-image/device lifetimes and independent quality/platform/hardware/human acceptance remain requirements of the full roadmap.

## Next ownership work, before whole-path closure

The preceding source inspection identified a wider cleanup boundary. `WorkerTree` in `worker_retirement.rs` marks process/cgroup retirement before its `TempDir` field is dropped; that destructor attempts deletion without propagating failure, then the resource reservation drops. `SupervisedNativeDependencies::close` in `native_capture.rs` returns image/staging cleanup errors, but its local storage reservation also drops on the error return. The new live-preparation failure path retains ownership on unconfirmed deletion; successful transfer into the existing worker custody does not establish that stronger guarantee for subsequent worker cleanup. The actual native worker cleanup failure is now reproduced and repaired in [checked staging cleanup custody](PULSAR_STAGING_CLEANUP_CUSTODY.md), with native-image/pre-spawn and held-inode replacement cases. The shared in-process cleanup owner does not establish durable staging recovery after process death.

- **Goal:** retain staging/native-image capacity and engine-scope ownership until their deletion is confirmed, including after inference retirement and engine restart.
- **Claim:** a cleanup error cannot make still-existing bytes unaccounted, and process-tree retirement alone cannot clear an outstanding staging obligation.
- **Deliverable:** shared explicit staging custody, checked cleanup transitions, creation intent and identity-bound durable recovery integrated with ordinary/live/native-capture ownership. Select the journal and reader barrier only after reviewing existing binding and portable-reader contracts.
- **Gate:** first reproduce cleanup failure with owned authored bytes; retain the failing case. Verify failure holds storage/scope, successful retry releases once, cancellation/late reply preserve ownership, and restart refuses or recovers exact owned identities without deleting unrelated state. Test creation-before-publication crash windows and corrupted/missing/mismatched records independently.
- **Means:** inspect `WorkerTree::retired`, `Custody::drop`, `SupervisedNativeDependencies::close`, native capture's `Retiring`, `AttemptCgroup::create`, and `WorkerFence` recovery. Reuse held-directory identities; no broad temporary-directory sweep or unrelated-process signaling.
- **Provenance:** retain current controlled-read packet as the preceding subject; create a separate source/binary packet for any ownership change.
- **Rollback:** preserve source and failure receipts. Refuse affected admission when durable ownership cannot be established; retain unresolved resources rather than report cleanup success.
