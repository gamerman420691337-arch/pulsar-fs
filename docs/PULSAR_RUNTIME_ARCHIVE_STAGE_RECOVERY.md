# Explicit archive construction-stage recovery

New full and changed-payload archive producers use private `.pulsar-runtime-archive-stage-*` directories with a synchronized locked receipt and private `archive.part`. A running producer holds the receipt lock through publication. Successful publication synchronizes the archive, stage and parent, removes its construction stage and synchronizes the parent again. This does not constitute power-loss qualification.

Inspect a known stage without deleting anything:

```sh
pulsar runtime-archive-stage-inspect /absolute/.pulsar-runtime-archive-stage-ID
```

A live writer reports `locked` without a discard identity. An inactive recognized stage reports `discardable`, its expected/present bytes, destination basename and `stage_sha256`. The inspection hash binds the exact receipt bytes, stage and receipt inodes, and presence/identity/length/change timestamps/mode/owner/link count of the partial file. It is an inspection identity, not an archive integrity or publisher signature.

Discard requires an explicit fresh identity and approval:

```sh
pulsar runtime-archive-stage-discard /absolute/.pulsar-runtime-archive-stage-ID \
  --expected-stage-sha256 INSPECTION_SHA256 --approve-discard
```

The command refuses live locks, stale identities, renamed/copied stage receipts, symlinks, hardlinks, wrong ownership/modes, oversized partial files and unknown contents before deleting known payloads. It removes only recognized names through held parents and rechecks entry identities and metadata. Concurrent external mutation or failed removal/synchronization may require renewed inspection; unknown entries are never recursively deleted by this command. There is no automatic scavenging or activation.

Legacy opaque `.pulsar-runtime-archive-*` files, absent/incomplete/corrupt receipts and unrecognized stages still need manual inspection. This command does not delete published archives, runtime installations or user project data. Archive validity, signed update trust, data-schema rollback, quota/retention, real filesystem/power-loss/platform and full model/media/device/human/edition qualification remain separate. Evidence is in `evidence/roadmap-2026-10-11/runtime-archive-stage/checkpoint.json`; full roadmap remains PARTIAL.
