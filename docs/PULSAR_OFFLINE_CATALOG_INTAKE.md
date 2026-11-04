# Owner-authorized offline catalog intake

Status: Linux engine/CLI integration implemented; [source-bound results](evidence/roadmap-2026-10-01/offline-catalog-intake/validation.json) retained with initial and failing capacity-regression artifacts. [Installed-pack association](PULSAR_INSTALLED_CATALOG_ASSOCIATION.md) is implemented separately; current publisher trust, revocation/update policy, desktop controls and release qualification remain open.

```sh
pulsar model catalog-refresh ABSOLUTE_REPOSITORY ABSOLUTE_BOOTSTRAP_JSON BOOTSTRAP_SHA256 --expected-revision 0 --approve-bootstrap --request-id retained-refresh-id
pulsar model catalog-status BOOTSTRAP_SHA256
```

A fresh paired session and the separate protected `model-pack-owner.token` capability are required before source or catalog storage access. Other owner capabilities do not substitute. The explicit approval binds the exact bootstrap bytes supplied out of band; a root accompanying a supplier pack does not automatically establish publisher identity. The namespace is the bootstrap SHA-256. A different root starts a separate explicitly approved namespace and does not replace or reset the old one.

Refresh reserves 256 MiB through engine admission and serializes against artifact effects. It claims the exact session/request fingerprint before filesystem effects, requires the expected catalog revision, captures only bounded local metadata/manifest files without following source directory ancestors or file links, and invokes the [offline verifier](PULSAR_OFFLINE_CATALOG_VERIFIER.md). Admission retains the verified current root, role versions and clock facts, including across repeated timestamp/snapshot key rotations. Catalog declarations do not load native libraries or select models.

The descriptor adapter permits at most 256 source files, 64 directories, four directory levels, 2 MiB per file and 32 MiB total. Root-level content is restricted to `metadata/` and `targets/`; path components are bounded portable ASCII. Symlinks, nonregular files, multiply linked files, unsafe components and oversized inputs refuse. The separately supplied public bootstrap is a bounded regular single-link file. These raw input/admission bounds do not qualify complete process memory or latency.

## Durable acceptance and replay

`STATE_DIR/model-catalogs` is private current-user 0700 storage. Namespace lock and snapshot files must be owned single-link 0600 regular files; links and unsafe modes refuse. Refresh takes a nonblocking advisory lock and rechecks its named inode before publication. The complete version-1 snapshot contains the approved bootstrap, current verified root, all metadata/clock watermarks, original exact manifests, acceptance report and historical request receipts. Binary objects use strict bounded canonical base64. The capsule SHA-256 detects accidental corruption; it does not grant publisher authority or protect against a malicious same-user owner.

Publication writes an exclusive private temporary file, syncs it, renames it through the held directory descriptor and syncs the directory. All trust facts and receipt intent become visible together. Subsequent SQLite response retention is separate. If a filesystem receipt exists while its SQLite response is missing, exact same-session/request replay returns the original report before re-reading supplier paths. Changed fingerprints refuse. Historical replay never replaces the newer snapshot or renews current trust. Failed verification preserves prior snapshot bytes.

Snapshots are capped at 64 MiB and retain at most 64 refresh receipts. Directory admission is capped at 128 entries, including lock files and interrupted temporaries; first acceptance reserves room for its snapshot after lock creation. Replacing a snapshot can temporarily add a staging entry. Capacity management, receipt retirement and explicit orphan cleanup remain required work; unknown state is not silently pruned. Advisory locking and protected cooperative local state are assumptions. Power-loss, disk-full, adversarial same-user filesystem and non-Linux qualification are not established here.

## Report scope

Status independently checks capsule integrity, private-file safety, root/role signatures and versions, original manifest hashes and signed target bindings. It returns the latest *stored historical acceptance report*. Metadata is not re-fetched and expiry is not used to fabricate renewed authentication. `authenticated_at_acceptance` is true; `current_publisher_trust`, `native_execution_approved`, `rights_verified`, `compatible` and `qualified` remain false. A report identifies the original accepting session/request and exact catalog/root/role revisions. No generation grant, job, installed asset, selection, physical output or official publisher approval follows.

Database schema 13 and portable format 9 are unchanged; the new commands and filesystem namespace add no existing selection semantics. Older-reader/platform release qualification remains separate. Native fixture keys are ephemeral and exist only in memory. The fixture exercises owner/approval refusal before storage, signed acceptance and rotation, restart, stale metadata/revision, expiry/corrupt targets, links and unsafe stored state, exact historical replay and preserved later revision. Removing the owned fixture's SQLite response specifically simulates a missing acknowledgement; it is not a crash inside a real transaction. No installed Stash, private reference media or Handy is contacted.

[Installed manifest association](PULSAR_INSTALLED_CATALOG_ASSOCIATION.md) now checks exact current catalog revision, retained signed original bytes, asset integrity and metadata expiry. Next integration adds explicit current approval/trust/revocation/rollback policy and desktop recovery, and completes rights, executable closure, provider KATs, working bundles and independent release gates.
