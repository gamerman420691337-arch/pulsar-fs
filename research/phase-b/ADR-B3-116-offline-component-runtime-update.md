# ADR-B3-116: offline component runtime update

Status: software implemented; native verification recorded separately; qualification open.

The runtime installer currently requires a full candidate. Add an explicit local host `runtime-update` operation accepting a delta directory, a retained base directory, a new destination and the expected base manifest SHA-256. The delta supplies a desired complete schema-1 candidate manifest and changed assets. Every omitted asset must match the base's declared path, length and digest; copied bytes are independently verified. A present corrupt, inaccessible or symbolic-link delta asset must fail rather than silently fall back.

Reuse the existing installer publication, disk admission, deadlines, streaming verification, nested generation-pack checks, durability reporting and inactive stage recovery. Capture both manifests and recheck them before publication. Produce a complete new installation with independent file copies, never hardlinks or mutation/removal of the base. Report provided/reused counts and bytes plus both manifest identities. No asset activation, application launch, daemon stop, secret copy or catalog/trust rollback occurs.

This is Linux x86-64 candidate component delivery, not signed automatic application update or Core/Complete release qualification. Retained old binaries support an explicit version choice; data-schema rollback compatibility is separate and cannot be inferred. Delta archives, deduplicated storage/quota/retention, active-job/power-loss/platform/human qualification remain open. See [workflow and native evidence](../../docs/PULSAR_RUNTIME_COMPONENT_UPDATES.md).
