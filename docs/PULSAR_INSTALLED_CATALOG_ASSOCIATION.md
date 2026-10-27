# Installed pack association with an accepted offline catalog

Status: Linux engine/CLI implemented and [source-bound local checks passed](evidence/roadmap-2026-10-01/installed-catalog-association/validation.json). This implements installed manifest association and point-in-time expiry checks. Execution trust, active update/rollback, desktop controls and release qualification remain open.

```sh
pulsar model pack-catalog-check BOOTSTRAP_SHA256 MANIFEST_SHA256 --expected-revision CATALOG_REVISION
```

The command requires a fresh paired session and the separate protected model-pack owner capability. It has no project, source-folder or approval argument and does not mutate storage. Engine artifact serialization and 256 MiB admission cover protected catalog reading and installed asset checks. No network request, model selection, native library load, job or physical output occurs.

The protected catalog capsule is independently checked using the existing [catalog intake](PULSAR_OFFLINE_CATALOG_INTAKE.md) integrity/signature/version/manifest reader. The caller must name its exact current stored revision. The original manifest must be listed as a signed target. Installed status rechecks all asset lengths/hashes, model/profile structural contracts and the inert receipt. The installed original manifest bytes must exactly equal the retained authenticated bytes; supplier reserialization is not substituted.

All four current metadata roles must remain unexpired at both the beginning and end of the asset check. Time must not predate the last accepted catalog clock watermark or move backwards during the check. Expiry uses the earliest root/timestamp/snapshot/targets deadline, with equality refused. This read-only check does not persist a new clock watermark, and cannot detect a clock rollback that stays after the last refresh watermark. Clock correctness and protected cooperative local state remain assumptions.

The client independently validates the returned bootstrap namespace, manifest digest and catalog revision against its request. A successful version-1 report includes the historical acceptance report, fresh installed integrity report, check time, earliest metadata expiry and `listed_in_catalog: true`. It always reports `online_revocation_checked: false`. The nested acceptance and pack reports preserve their false publisher-trust, native-execution, rights, compatibility, quality and activation claims. Association is an exact observed fact, not an approval or proof that an offline snapshot is the publisher's newest metadata.

When a later accepted signed catalog omits the manifest, this check refuses. The installed bytes remain available for explicit integrity inspection; existing unqualified owner selection is unchanged. Omission does not automatically revoke an existing approval or stop a job. [Explicit catalog-bound owner approval](PULSAR_CATALOG_BOUND_APPROVAL.md) now fences future admission under exact stored namespace/revision and unexpired metadata. Online publisher revocation, automatic in-flight cancellation, rollback retention and update transactions remain required integration. No unsigned local namespace or portable project can silently acquire those semantics.

This read-only increment left database schema 13 and portable format 9 unchanged. Subsequent catalog-bound approval conditionally raises the database reader barrier to 14. Broader reader/platform, power-loss/disk-full, native-window, model/media, Handy, human and release gates remain open. Authored ephemeral-key/native fixtures provide software evidence without using private reference media or installed Stash.
