# Retained batch preparation recovery

2026-10-12. Implemented scoped CLI recovery; full roadmap PARTIAL.

An interrupted publication preparation can leave record.json in Preparing after the original stage and a complete pending preparation receipt have been written. Previously the batch refused every subsequent publication attempt, preserving work but leaving no explicit reconciliation action. The new command attaches only that original captured preparation:

```sh
pulsar library-batch-recover-preparation BATCH \
  --publication-state /absolute/private/workflow \
  --index 0 \
  --staging /absolute/media/.pulsar-neutral-bundle-ORIGINAL \
  --preparation-receipt /absolute/private/workflow/.pending-ORIGINAL
```

The index is zero-based. The workflow directory must remain private and owned. The pending receipt must be a bounded, private, single-link regular file in that exact directory; links, external filenames, incomplete JSON, foreign credential bindings and changed metadata refuse. Recovery holds the existing workflow lock and reads current Engine generation status. It inspects the exact stage using existing owner, inode, request, current master, project/revision, member-byte and destination checks. Inspection does not reserve exports or verify unknown Engine export outcomes.

The captured pending receipt must equal the current retained journal with only the selected Preparing item transitioned to the inspected Prepared state, preserving the six original export request identities. A different entry, project, revision, destination, receipt scope or replacement request refuses before journal publication. An exact already-Prepared repeat performs no journal write. Recovery changes client metadata only; subsequent explicit publish uses the existing exact-intent publisher. Missing original pending evidence remains unresolved; a matching newly created stage alone is insufficient.

## Evidence and scope

[Checkpoint](evidence/roadmap-2026-10-12/library-preparation-recovery/checkpoint.json), physically retained in `/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-12/library-preparation-recovery/`. Client tests pass 373 with ten ignored. Three new tests cover original-identity attachment/idempotence, wrong entry/phase refusal, and private pending-receipt capture/refusal. Both strict workspace/all-target lint configurations, Windows GNU source check, native App/Worker/controller builds, formatting and architecture/source guards pass. Existing Windows warnings remain; no native Windows qualification or fresh whole Engine/Protocol runtime suite is claimed.

The actual network-disabled CLI campaign stops an owned client after successful fsync of its Prepared pending receipt and before record.json replacement, observes the stopped PID and current Preparing record, then kills and joins that exact client. Recovery dispatches zero exports. Exact repeat and Engine restart preserve the recovered journal bytes. Wrong-entry, automatic-replacement, changed pending export-ID and missing-receipt attempts preserve the original journal and Engine counts. Subsequent publication uses the original axis request IDs and all twelve scripts match the preceding authored PCM/timing oracles. Two clients are killed/joined and four native Engines are joined across the initial pass and final negative-control campaign.

A test-only shared-library helper intercepts successful fsync solely for Prepared pending records inside an explicit fixture directory; production has no crash hook. Source, helper identity, commands, pending records and original stage intents are retained. Application credentials exist only in owned temporary RAM. The first validation was interrupted by an external evidence-root move and failed with FileNotFoundError when writing checks.json; the failure is retained and final validation passes under the updated keep layout. Compatibility links preserve historical citations; no external move is reversed.

This is one specific Linux preparation acknowledgement/death window. Other crash/storage faults, absent pending receipts, standalone unknown export outcomes, [desktop recovery controls](PULSAR_DESKTOP_PREPARATION_RECOVERY.md) now have scoped controller evidence; rendered GUI acceptance, platform/model/media/device/human and release qualification remain open. Useful model defaults and missing training annotations are not supplied by metadata recovery.
