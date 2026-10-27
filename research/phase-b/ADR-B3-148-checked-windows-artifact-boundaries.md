# ADR B3-148: Do not invent Windows artifact privacy or durability

Status: implemented; scoped checks PASS. Full Windows storage/publication remains unsupported.

The application refuses unsecured Windows server startup. Its artifact library must not separately create a supposedly private directory without checking ownership/DACLs, or report a directory sync it did not perform. Non-Unix private-directory and synchronization helpers now return unavailability errors. High-level snapshot, opened-source capture, worker reuse and export paths check support before side effects, so a later sync refusal cannot conceal already-published output. Preserve existing Unix behavior and no-clobber publication.

Implement Windows supplier reads with safe native open options and held metadata checks: read only, no write/delete sharing, final-reparse opening, anonymous security QoS, native absolute filesystem prefix, device/UNC pipe refusal and regular non-reparse held file. Share no path-based authority or private-storage guarantee with this reader. Existing bounded digest behavior remains common.

The negative baseline uses preceding production definitions plus the new assertions. Three failures demonstrate real weak outcomes rather than a hypothetical Windows issue. The final scoped checks exercise checked reads and refusal-before-effects. Restore final source before continuing normal builds; retain both subjects and their executable identities. Wine is emulation and cannot qualify native Windows ACLs, anchoring, filesystem durability or platform release behavior.

Final scoped Wine checks pass 3 tests; 541 other Windows Engine tests remain filtered out. Rebuilt Linux Engine/Clients pass 841/322 with 70/8 ignored, along with both strict Linux lint configurations, Linux/Windows all-target checks, formatting, native build and architecture/source guards. This closes the demonstrated false-guarantee bugs without claiming Windows storage or publication completion.

See [scope and primary API references](../../docs/PULSAR_WINDOWS_ARTIFACT_BOUNDARIES.md). Independent product/model/media/device/human/release gates remain open.
