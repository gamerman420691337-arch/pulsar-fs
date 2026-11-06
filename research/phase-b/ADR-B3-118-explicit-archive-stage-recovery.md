# ADR-B3-118: explicit archive construction-stage recovery

Status: software implemented; verification recorded separately; qualification open.

A process killed while packing leaves an opaque temporary file. Both full and changed-payload archive producers should use a private construction directory containing a locked durable strict receipt and private partial archive. Publish only completed verified bytes with atomic no-overwrite rename. Preserve existing publication uncertainty reporting and synchronize successful stage cleanup.

Add trusted local host inspection and explicit approved discard. Bind the inspection hash to receipt bytes and held stage/receipt identities plus partial-file identity and change metadata. Refuse stale approvals, live writers, unexpected entries and untrusted ownership/modes/link counts. Remove only recognized names through held parent descriptors, verifying identity and metadata before each operation. Do not automatically scavenge archives or remove published destinations, installed runtimes or project data.

Legacy opaque files and incomplete/unrecognized receipts are not silently adopted. Failed deletion or external concurrent mutation may leave a partial stage requiring manual inspection. Controlled SIGSTOP/SIGKILL tests establish process-death behavior; they do not qualify power loss or the full storage/platform/device/model/media/human release matrix. See [workflow and evidence](../../docs/PULSAR_RUNTIME_ARCHIVE_STAGE_RECOVERY.md).
