# ADR B3-198: Retain an explicitly approved original gap commit before effects

Persist the exact candidate/base/program pin and original request after current
review and explicit acknowledgement, before Engine dispatch. Recover that original
intent for explicit replay under current Engine authority and a private workflow
lock; validate its candidate against authenticated readback. Checkpoint the durable
acknowledgement and distinguish original acknowledged revision from current master.

Do not synthesize approval for legacy upload-only records or restore a review pin.
Do not replace unknown commit identities. A new approval still requires current
diagnostics and acknowledgement. Original recording/export recovery, managed
retirement and broader filesystem/platform/physical qualification remain required.

See [contract and evidence](../../docs/PULSAR_BROWSER_GAP_COMMIT_RECOVERY.md).
