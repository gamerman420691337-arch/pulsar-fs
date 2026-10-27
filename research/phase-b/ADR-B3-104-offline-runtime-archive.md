# ADR B3-104: single-file offline runtime candidate archive

Status: implemented; verification recorded in the source-bound checkpoint. Scope: Linux x86-64 local candidate packaging/extraction before Engine configuration.

Problem: directory-only delivery requires keeping fifteen declared payloads and their original manifest together. A single offline file makes candidate transport and inspection concrete.

Decision: expose runtime-pack SOURCE_DIRECTORY NEW_ARCHIVE and admit either a directory or archive through runtime-install SOURCE NEW_DESTINATION. Version 1 is uncompressed: twelve ASCII bytes PULSARRT0001, a four-byte little-endian manifest length, the original schema-1 manifest JSON, then raw payloads in manifest-record order. The manifest is bounded at 64 KiB and retains qualified:false. Existing finite membership, per-file and total byte admission remains authoritative. No arbitrary archive paths, link records, external extractor, inference or automatic model activation are introduced.

Read through held no-follow descriptors, stream through 64 KiB buffers and verify every exact length/hash and stable metadata. Reject unsupported versions, truncated data, trailing data and late payload corruption. Packaging uses an owned private temporary file, file synchronization and atomic no-overwrite publication with explicit parent-sync uncertainty. Extraction uses the existing private locked stage, original-manifest receipt, synchronization/publication and explicit recognized-stage recovery.

A killed packer can leave an opaque .pulsar-runtime-archive-* temporary file. Directory stage recovery does not recognize it; automatic archive-temp cleanup remains open. The ten-minute between-operation deadline cannot interrupt a blocked kernel filesystem operation. Same-user arbitrary OS mutation and exhaustive power-loss/filesystem faults are outside the demonstrated boundary.

This format provides declared-byte integrity, not signature authenticity or license/model/platform qualification. Core/Complete generation assets/defaults, first-launch embedded extraction, actual AppImage/Windows/macOS delivery, signed updates/rollback, useful-model/media/device and human/release gates remain open.
