# ADR B3-039: Staging obligations share the worker ownership ledger

Status: implemented for tracked live-inference staging; broader acceptance pending.

The owner-death regression demonstrated that process-local cleanup custody can disappear while staged bytes remain. An independent staging file would allow an old worker-fence reader to ignore that ownership. The schema-19 reader prerequisite and admitted-creation regression establish the required publication and resource-retention order.

Use canonical ledger format 3 with a required staging map in the existing leased scope. Upgrade the ledger before committing schema 19, then create through the admitted guard only after admission commits. Both older readers and a current engine presented with a downgraded ledger must refuse affected state. Preserve canonical format-2 compatibility for subjects that never declare staging support; do not infer that this proves their historical temporary roots are clean.

Keep new staging roots directly in the private fence namespace. Preflight all identities before any recovery deletion, refuse unrecorded names in this namespace, and retain retirement tombstones until exact absence and durable clear. Transfer successful creation into worker custody; cancellation and handle Drop are not byte-user retirement proof.

The actual new process-kill case verifies the production primitive under failed deletion and restored recovery. Full engine crash cuts, legacy migration, native images and cgroup creation intent remain separate required work. See the source-bound [implementation record](../../docs/PULSAR_STAGING_FORMAT3.md).
