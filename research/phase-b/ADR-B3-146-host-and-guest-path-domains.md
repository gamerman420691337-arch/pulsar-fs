# ADR B3-146: Keep host paths and guest coordinates in their own namespaces

Status: implemented and scoped checks PASS. Native Windows qualification remains open.

NativeCaptureRequest declares fixed POSIX guest coordinates. Calling ordinary NativeDependencySpec host-path validation first rejects these valid declarations on Windows. Share dependency schema, hash, name, identity and byte-budget checks, then select the appropriate private path validator. Ordinary dependency suppliers retain native host absolute-path validation. Capture inputs require exact UTF-8 `/inputs/{index}` strings on every host; slash aliases, dot components, backslashes, mismatched indices and supplier paths refuse. Valid Linux request serialization and identity remain unchanged. No Windows native capture or confinement implementation is enabled.

Bundled discovery presentation also rejects valid Windows absolute paths because its normalized component allowlist omits the drive/UNC prefix. Admit native Prefix components alongside RootDir and Normal only after native is_absolute succeeds. Retain digest-named directory, exact generation child path, bounded inventory, alias/duplicate/authority and parent-traversal checks. These are presentation checks, not filesystem intake or execution authority.

Use native temporary-directory coordinates for authored positive host-path fixtures in Clients and Protocol. Keep Stash server roots and native capture guest coordinates in POSIX namespaces. Preserve traversal/alias controls and original wire fixture roundtrips. The two real retained-handle package positives remain Unix-scoped; a non-Unix test checks explicit unsupported refusal, unchanged source, idle progress and absent additional files. This does not implement Windows package import. Compare a canonicalized script-output parent with the canonicalized expected parent, retaining no-clobber and source-alias refusal.

Isolated Wine execution passes all 38 executables (626 passed, 3 ignored). Linux Core/Protocol, rebuilt Engine/Clients, both strict Linux lint configurations, workspace checks, formatting, native build and architecture/source guards pass. Retain the initial failures and their corrections, including the drawn test pointer targeting clipped long paths. Windows target lint warnings remain open.

See [verification scope](../../docs/PULSAR_WINDOWS_PORTABLE_FIXTURES.md). All product, model, media, hardware, native OS, packaging and human acceptance gates remain independent.
