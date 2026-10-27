# Structured authored motion-retiming receipts

New `RetimeMotion` edits now commit a version-1 receipt in the same SQLite transaction as the revision, motion/history change and request acknowledgment. A failed receipt insert rolls back the edit. Immutable motion-store files published during a failed transaction may remain inert orphans; they do not become a committed project revision.

```text
pulsar project retime-status PROJECT --revision RETIME_REVISION
```

Status requires the current authenticated caller's Read authority on this project, not the original editor's current grant. It reports recorded facts about a historical edit; it does not grant editing, export, device or release authority.

The strict receipt binds project/base/result revision, session/request, recorded actor and operation fingerprint, complete retained old/new placements, explicit selected axes, before/after immutable motion identities and recording time. The fingerprint is a digest; no authentication credential is stored in the receipt. Source clocks and artifact identity are declarations about this authored edit, not measurements of real media or physical motion.

Status bounds receipt and request-response bytes before Rust allocation, verifies the stored receipt digest and strict contract, checks revision/lineage kind and parent, actor/session attribution, request fingerprint and original cached Project response, retained placement definitions and historical program identities. It checks that the target placement is in the original response's active membership. Under an explicit memory reservation it reads both verified motion objects and recomputes the retime, requiring complete program equality. A matching checksum alone is insufficient. Oversized/corrupted receipt cells, changed attribution or a different transformation refuse. Current code/receipt algorithm semantics are version 1; any incompatible change must preserve validation for this version or explicitly refuse it.

## Schema and migration

Project database schema 6 adds `motion_retime_receipts`, keyed by `(project,revision)` with a composite foreign key to the recorded revision. Startup checks schema-6 column, primary-key and foreign-key layout before accepting it. Prior supported schemas migrate through the existing placement initialization; new receipt creation and the version increase share its transaction. Older engine readers refuse schema 6. Current startup inspects an existing database schema/layout through a read-only SQLite connection before opening it for writable initialization.

Schema-5 retiming edits did not record structured receipts. Migration does not reconstruct axes/intent from human-readable descriptions or fabricate receipts for them; status remains NotFound. The exact schema-5 engine binary was frozen before this change. Native validation verifies its SHA-256, refusal of a quiescent, checkpointed schema-6 subject without changing the project database file, and successful current-engine reopening with the original retime receipt intact.

[Portable format 5](PULSAR_PORTABLE_RETIMING_RECEIPTS.md) now retains verified native receipt facts and recomputes their transformations during import. Clones preserve originals in digest-bound origin manifests and namespace mappings rather than fabricating destination-local authenticated receipts. Archival status and executable clock correspondence across clone namespaces remain implementation work; a clone must not acquire authority through a copied description. [Verified source-clock composition and explicit Stash publication](PULSAR_RETIMED_STASH_PUBLICATION.md) now support ordered single-clock histories; mixed-history correspondence and portable Stash association/proof integration remain open.

The first native historical-reader run compared a non-quiescent main file while committed pages remained in WAL. The frozen schema-5 reader refused the future schema but its writable SQLite connection checkpointed those pages, changing the main-file hash; inspection still found schema 6, the receipt and the expected project revision. The fixture now checkpoints its owned subject before checking historical main-file identity. Separately, current startup is tested against an unknown schema committed only in WAL, requiring exact main-file and WAL hashes to survive refusal. SQLite documents committed WAL contents and checkpoint-on-last-close behavior in its [WAL reference](https://www.sqlite.org/wal.html). This is a current-reader repair; the frozen historical binary is preserved unchanged.

## Evidence boundary

Protocol tests exercise strict source/revision/axis/artifact bindings and scoped read-only command behavior. Authenticated-engine cases cover failed-insert rollback, exact replay, Read-only receipt access, oversized/corrupted receipt refusal and restart. Native CLI cases bind before/after identities, preserve the receipt through undo/redo/restart, reject a rehashed forged actor, restore the original receipt and exercise the pinned schema-5 refusal boundary. Separate layout tests reject missing foreign keys and reversed composite primary keys.

These fixtures contain authored script values and an inert, never-decoded source. They do not prove real-media alignment, actor identity outside the engine, cryptographic nonrepudiation, physical Handy synchronization, model quality, platform release qualification or full roadmap completion. Exact source/binary identities and raw final logs are recorded in the increment evidence directory.
