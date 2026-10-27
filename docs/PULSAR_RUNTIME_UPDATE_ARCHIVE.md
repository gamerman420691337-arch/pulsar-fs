# Offline changed-payload runtime archive

Linux x86-64 hosts can package a complete candidate against an exact retained base:

```sh
pulsar runtime-update-pack /absolute/complete-new /absolute/base /absolute/changed.pulsar-update \
  --expected-base-manifest-sha256 LOWERCASE_BASE_SHA256
pulsar runtime-update /absolute/changed.pulsar-update /absolute/base /absolute/new-installation \
  --expected-base-manifest-sha256 LOWERCASE_BASE_SHA256
```

The producer verifies every supplied candidate asset, including unchanged files it omits. It preserves the exact desired manifest bytes and emits changed payloads in declared manifest order. It writes a private new file with no-overwrite publication and synchronized bytes and parent directory. The base manifest pin is mandatory.

The uncompressed format starts with the 12-byte magic `PULSARUP0001`, a little-endian 32-bit descriptor length, a UTF-8 JSON descriptor and changed payloads. The descriptor contains `base_manifest_sha256`, the exact UTF-8 `desired_manifest` string and `provided_paths`. The descriptor is bounded to 262144 bytes and its embedded manifest to 65536 bytes. Unknown fields, undeclared/duplicate/reordered paths, inconsistent length, truncation and trailing bytes refuse. Each provided payload has its desired manifest length and SHA-256. Omitted records must match the explicitly pinned base; the installer independently verifies their actual copied bytes.

A changed-payload archive requires `runtime-update`; ordinary unbound `runtime-install` refuses it. It produces a complete independent new runtime, preserving the base and original desired manifest. The archive does not activate a model, switch a running process or restore old trust, clocks, catalog revisions or project schemas. Retained application binaries alone do not establish data rollback compatibility.

Evidence is recorded separately in `evidence/roadmap-2026-10-11/runtime-update-archive/checkpoint.json`. [Explicit construction-stage recovery](PULSAR_RUNTIME_ARCHIVE_STAGE_RECOVERY.md) handles recognized inactive interrupted packers. Signed publisher/update trust, deduplication, quotas/retention, active-job/power-loss/filesystem qualification, actual Core/Complete models/defaults, OS/native-window/physical-device/human and release gates remain open. Full roadmap status remains PARTIAL.
