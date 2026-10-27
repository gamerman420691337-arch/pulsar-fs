# ADR B3-147: Retain Windows package input with checked open semantics

Status: implemented; scoped diagnostic checks PASS. Native Windows and Engine publication qualification remain open.

Windows is a primary launch platform. The retained package Client previously refused all non-Unix input. Add a Windows implementation behind cfg(windows), preserving the other-platform refusal and existing Unix flags. Use safe Rust Windows OpenOptionsExt only: read access, no write/delete sharing, open the final reparse point itself, anonymous security QoS, then check the actual held handle's regular-file/reparse attributes. Restrict input to absolute filesystem prefixes and reject device/UNC pipe namespaces before opening. Preserve normal and extended UNC filesystem paths without claiming SMB qualification.

Keep all package hash/version/budget/cancellation/deadline/rewind logic shared. The declaration carries exact bytes identity, never a selected path or filesystem authority. Retain the opened File across replay and upload. This does not enable Windows Engine private storage, archive validation or publication by itself.

Extend the existing exact-byte preparation and two real retained-source controller/recovery tests to Windows; they still require actual checked File intake and preserve ambiguous Start identity. Add actual Windows API checks for writer conflict, hard-link sharing, rename/delete refusal, drop/release behavior and genuine reparse-point rejection before hashing. Existing Unix pathname-replacement and no-follow tests remain scoped to Unix behavior. Wine is diagnostic emulation, not native qualification.

Final Wine verification passes all 38 portable executables (630 tests, 3 ignored), including 207 Client tests. Rebuilt Linux Clients pass 322 with 8 ignored; both strict Linux lint configurations, Linux/Windows all-target source checks, formatting, native build and architecture/source guards pass. Windows target warnings and native/MSVC qualification remain open.

See [scope and primary API references](../../docs/PULSAR_WINDOWS_PACKAGE_INPUT.md). Product/platform/model/media/device/human/release gates remain independent.
