# ADR-B3-117: offline changed-payload runtime archive

Status: software implemented; verification recorded separately; qualification open.

Directory component delivery requires transferring files separately. Add an explicit local `runtime-update-pack` producer taking a complete desired candidate, a retained base, a new archive path and an exact expected base-manifest SHA-256. Retain exact desired manifest bytes and transport only records whose path, length or hash differ from the base. Verify all producer source bytes, including omitted unchanged assets.

Use a separate versioned uncompressed format with bounded strict descriptor fields, ordered unique declared payload paths and exact archive length. Admit it only through pinned `runtime-update`. Stream each changed payload through existing length/hash/deadline checks; independently copy and verify reused assets. Reuse private durable no-overwrite archive and installation publication. Check captured archive metadata and EOF before publication and recheck held base manifest bytes.

No signed publisher identity or automated trust is inferred from a local base pin or payload hashes. A complete independent copied installation and retained old binaries do not prove project-data rollback compatibility. No runtime process, model selection, catalog/trust state or credentials are changed. See [format and native evidence](../../docs/PULSAR_RUNTIME_UPDATE_ARCHIVE.md). Full edition/platform/model/media/device/human qualification remains open.
