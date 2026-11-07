# Handy public RPC stop codec

Status: **IMPLEMENTED / focused checks PASS / device qualification UNKNOWN / roadmap PARTIAL**.

The user selected Handy 2 Pro and has not configured encrypted credential environment names. Observed firmware remains unknown. The [manufacturer contract refresh](../research/phase-b/handy2-pro-fw4-contract-refresh-2026-10-09.md) found a public draft RPC schema at [commit 7158570563bdcd6de356fff62e3f8c159189a6d2](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/tree/7158570563bdcd6de356fff62e3f8c159189a6d2). It provides an application message schema; retrieved sources do not establish the owning FW4 BLE framing, security/session, fragmentation, watchdog or measured stop contract. The manufacturer marks the protocol draft. Compatibility with the user's device is unverified.

The new dependency-free Engine `handy_rpc` module encodes explicit current-mode, HDSP and HSP stop families and decodes their response subset. Successful replies require the exact request ID; HSP additionally requires its typed result. Current-mode and HDSP generic responses have no command discriminator, so a future transport must bind outstanding IDs to requests and sessions. Remote error presence refuses without returning peer content. The module does not create sessions, write to hardware, grant admission or change device capabilities.

The strict decoder refuses malformed, truncated, duplicate, unknown, noncanonical and oversized messages, validates bounded HSP scalar fields and finite floats, and exposes no motion state as physical stopped proof. Its 64 KiB receive bound, 64-field bound and nonzero request-ID reservation are local codec policy, not vendor MTU or physical safety guarantees. A successfully decoded acknowledgment is only an RPC reply.

## Verification

Local `protoc` and Python Protobuf independently serialized 18 request/reply cases from the pinned manufacturer schema: six IDs spanning varint and uint32 boundaries, each with all three stop families. HSP replies exercise signed values, floats, booleans and maximum stream ID. Authored numerical fixtures are retained in the Engine test tree; upstream schema/generated library files are not vendored. The manufacturer tree has no retrieved license grant; redistribution terms remain UNKNOWN. Temporary upstream files were used only for the oracle and removed with owned scratch.

All six codec tests PASS in the full default Engine test binary, including all 18 oracle cases and wrong-ID/family/error/duplicate/truncation/overflow/typed-state controls. All nine neighboring Handy REST wire tests PASS. Core, protocol and default Engine compilation PASS. Standalone codec compilation/tests and its rustfmt check PASS. Architecture: 294 checks PASS. Source guards: 36 tests PASS. A repeat oracle generation timed out at 10 seconds; its 55-second bounded retry PASS and reproduced all 18 retained cases. The timeout remains recorded.

The source inventory binds 424 records: only existing `lib.rs` changed, one new codec module and one new fixture were added, and 421 existing records are unchanged. Full Engine/workspace suites, CUDA builds, release binaries and packaging were not rerun for this source. No device discovery, authenticated API request, physical playback, private-media access, training or human acceptance occurred.

[Terminal command receipts, own fixtures/harness, pinned upstream identities, source snapshots/diff and file manifest](evidence/roadmap-2026-10-09/handy-rpc-stop-codec/checkpoint.json) are retained. Owned binaries and their command handles were terminal before cleanup. The process scan found no accessible executable, current-directory or descriptor reference to scratch; 1,356 executable/current-directory entries were permission-inaccessible, so it is not a complete host process audit. Shared caches/services were untouched. The next hardware gates remain observed firmware, encrypted credentials for the selected online path, owning transport/session contract, authority and scheduling integration, disconnect/recovery behavior, measured stopping and human acceptance. Full roadmap remains PARTIAL.


## Successor — 2026-10-09

[RPC error correlation and mixed-clock runtime](PULSAR_HANDY_RPC_ERROR_CORRELATION.md) records the corrected error decoder and the first completed mixed-clock export regression, including all four negative/restoration controls. Historical results above retain their original scope. Runtime reliability, public integration and the complete roadmap remain open.
