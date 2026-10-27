# Authenticated WASM editing

Status: verified local Linux editor workflow; GB-12 and the roadmap remain incomplete.

The optional Linux WASM build exposes `WasmEditorInfo`, `StartWasmEdit`, `WasmEditStatus` and `CancelWasmEdit` through the authenticated engine protocol. A start needs both Read and Edit grants, the current project revision, an Editor manifest without effect scopes, explicit approval for this invocation, the module bytes and the expected native worker identity. The engine binds that approval to its startup executable identity and compares it again against the sealed executable captured before launch. A mismatch refuses execution.

The guest receives only values from the captured project revision. The whole-program input and output each fit the existing 64 KiB ABI; larger projects are refused without truncation. The module fits the 512 KiB worker bound and the serialized control-frame admission. There are no host imports, project paths, device handles or secret mounts. Existing Linux supervision enforces worker admission, durable custody, process-tree retirement, fuel and resource bounds. The engine also reserves the existing 1 GiB edit-validation allowance for reconciliation and publication.

Output is an untrusted values-only edit. The core reconciler preserves matching base evidence and marks authored changes Synthesized. The result creates an uncommitted candidate with the existing core edit receipt, lineage and review flags. A separate candidate commit applies the existing protected-region rules and automatic manual protection; undo uses the existing revision history. Plugin output cannot declare observations or replace evidence metadata.

The engine checks authorization, revision, epoch and active operation state before execution and again inside the candidate-publication transaction. Cancellation and grant revocation terminate active admission; restoring a grant cannot revive the operation. Restart marks running operations Interrupted and requires a fresh approved invocation. Request IDs support exact replay and reject different requests under the same identity. Status and cancellation are restricted to the initiating session and project.

Retained records bind the module, native executable, input/output bytes, base/result descriptors and core receipt. Startup checks table constraints, bounded metadata and byte fields, state shape and retained associations before writable startup effects. There are at most 64 retained operations globally. Removal/eviction of these records is still needed; exhaustion refuses new work.

The CLI provides:

```text
pulsar wasm-edit info PROJECT
pulsar wasm-edit start PROJECT MODULE --manifest MANIFEST --revision REVISION \
  --worker-sha256 EXPECTED_SHA256 --approve-execution
pulsar wasm-edit status PROJECT ATTEMPT
pulsar wasm-edit cancel PROJECT ATTEMPT
```

Use `--request-id` on start or cancel when an explicit replay identity is needed. Start returns an operation; poll status for its candidate or terminal error. The CLI checks engine response identity and invocation correspondence. Engine pairing and grants use the existing client setup.

[Portable invocation custody](PULSAR_PORTABLE_WASM_INVOCATIONS.md) now retains the module, original input/output, resource/approval declarations and receipt in inert format-12 archives, with checked clone/restart/re-export and schema-25 reader barriers. Native executable/runtime bytes remain absent. Remaining work includes GUI approval/status controls, catalog cleanup, processor and native plugin roles, immutable native runtime dependencies and broader adversarial/crash/resource qualification. Host filesystem/fence/cleanup calls are not proven deadline-preemptible. Native runtime qualification remains false. Real media/model quality, platform, Handy and human/release gates remain open.

## Verification — 2026-10-10

The [checkpoint](evidence/roadmap-2026-10-10/authenticated-wasm-editing/checkpoint.json) records exact source and executable identities. Protocol contracts pass 2/2; CLI grammar/file-admission tests pass 2/2; Engine WASM/runtime/capture/catalog tests pass 13/13 with two delegated cases default-ignored. The new authenticated fixture was explicitly run and passes 1/1 with zero ignored. It executes an authored real guest through engine authorization, candidate reconciliation and commit/undo, checks synthesized evidence and exact replay/conflict, rejects incorrect approval/module/worker/revision before operation creation, denies a Read-only caller and another session's status access, and refuses late publication after cancellation, revision change and revocation followed by legitimate grant restoration. Full parent resource admission returns after each case; no owned worker cgroups remain.

Both default and WASM application checks pass with 37 existing Engine warnings; corrected test compilation has 27 warnings. Architecture checks pass 310/310; source guards pass 36/36; formatting passes for the six owned modules/tests. This is a targeted increment, not a full workspace/CUDA/Clippy, IPC-socket, rendered GUI, native-runtime or crash campaign.

The successful authenticated fixture took 181.50 seconds. Its nested guest limits do not establish hard host-filesystem/fence/publication/cleanup deadlines or product performance. The initial 72.69-second runtime attempt failed because the fixture's administrator lacked Edit permission when restoring that grant; granting Edit to the fixture administrator corrected the test without weakening production authorization. A disconnected test barrier then exercised panic cleanup during the failed attempt; the attempt is retained as a failure, not promoted to qualification. Initial compile diagnostics and an interrupted own Cargo-cache wait are retained separately.

[Catalog discovery and unsuccessful-attempt removal](PULSAR_WASM_CATALOG_RECOVERY.md) now recover retained original request IDs and explicitly remove retired failed/cancelled/interrupted operations under current Read+Edit authority. Completed invocation bytes remain protected and still count against the 64-operation limit; separate durable candidate evidence and broader retention policy remain open.

[Desktop WASM workflow](PULSAR_DESKTOP_WASM_EDITOR.md) now connects captured declaration inspection, fresh per-invocation approval, identified request recovery, status/cancellation and existing candidate review controls. Controller/native IPC checks pass; [durable local recovery](PULSAR_DESKTOP_WASM_RECOVERY.md) now preserves original request metadata with fresh approval required for retry. [Unsuccessful-attempt cleanup](PULSAR_DESKTOP_WASM_CLEANUP.md) now connects the engine removal contract to the desktop controller. [Durable cleanup recovery](PULSAR_DESKTOP_WASM_CLEANUP_RECOVERY.md) now retains original cleanup identity across fresh controllers without automatic dispatch. Actual window rendering/usability, authoritative unknown-request resolution and broader crash/quota qualification remain open.

[Completed invocation archival](PULSAR_WASM_COMPLETED_ARCHIVAL.md) now separates bounded operational visibility from retained original bytes/receipts and portable lineage. Explicit archival frees a catalog slot with current authority and retired custody; total retained invocation capacity remains bounded at 256. Desktop archival presentation/recovery and retained-evidence disposal/quota qualification remain open.
