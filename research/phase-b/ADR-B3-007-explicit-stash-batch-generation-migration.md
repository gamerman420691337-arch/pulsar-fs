# ADR B3-007: Explicit future-generation batch migration

Status: implemented locally; broader B3 and qualification remain open.

A captured model or engine/tool/owner environment cannot silently change after batch selection. Matching restoration recovers original intent but cannot express a deliberate replacement. Add a typed full generation replacement command under current owning-session/local-owner authority and an exact observed private-ledger digest.

Require a stopped attention batch, reject active schedulers and use raw-record compare-and-swap plus an atomic authority replay receipt. Capture the explicit replacement before publication; reserve bounded metadata and refuse a SQLite record larger than 16 MiB before Rust allocation. Migration never starts scheduling; resume remains separate. Future projects require current Generate grants and the retained revision.

Reconcile successful original generation receipts inside the authority transaction. Preserve accepted jobs and their manifests; only an unaccepted pending generation may become superseded private history. Preserve all original phase IDs, previous options/owner digest and distinct reconciled/superseded receipt lists. Never clear or redispatch uncertain external phases. Limit migration history to 16 and preserve bounded status pages.

Migrated private records carry report version 3 with migration receipts. The previous strict report reader rejects the new field instead of erasing history through an older serializer. Retain schema 9 and portable maximum 7; native previous-reader status/resume refusals leave ledger/master unchanged. Unmigrated version-2 records remain decodable.

See [contract and exact local evidence](../../docs/PULSAR_STASH_BATCH_MIGRATION.md). No accepted-job replacement, portable Stash-association migration, lost remote identity recovery, actual Stash, model/media, Handy, GUI/platform/human or release qualification is implied.
