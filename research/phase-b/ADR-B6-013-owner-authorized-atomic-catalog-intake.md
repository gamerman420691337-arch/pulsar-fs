# ADR B6-013: Publish catalog watermarks and replay facts atomically

Status: implemented Linux engine/CLI integration; broader trust/update qualification incomplete.

Require a fresh paired session, separate host-local model-pack owner capability, exact bootstrap hash, explicit approval and optimistic catalog revision before accepting supplier metadata. Namespace by the approved bootstrap so a new root does not reset an existing namespace. Capture local bytes through bounded no-follow descriptors; use the existing maintained verifier with protected current-root/role/clock state.

Store one private atomic capsule per namespace containing all trust facts, original manifests and exact actor/request/fingerprint reports. Use owned 0700 directories and single-link 0600 files, named-inode-checked cooperative locking, bounded canonical base64, synced exclusive staging and descriptor-relative rename/directory sync. Claim SQLite request identity before filesystem effects; retain the response afterward. A capsule receipt resolves an accepted operation with missing SQL response without reading changed supplier inputs or replacing newer state.

Historical acceptance is distinct from current publisher trust. Replayed/status reports cannot approve native code, rights, model quality or a fresh trusted update. Check retained signed target/manifest bindings independently. Refuse corrupt, unsafe, stale, oversized and full storage without replacing the working capsule. Directory admission reserves the first capsule slot; no automatic unknown-outcome pruning or orphan cleanup.

Use filesystem snapshot schema 1; database schema 13 and portable format 9 are unchanged. Installed-pack association is implemented separately in ADR B6-014. Desktop workflows, active trust/revocation/rollback, retention management and platform/power-loss/human/release qualification remain open. See ../../docs/PULSAR_OFFLINE_CATALOG_INTAKE.md.
