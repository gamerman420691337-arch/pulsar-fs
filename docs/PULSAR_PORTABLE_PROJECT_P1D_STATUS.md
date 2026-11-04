> Historical checkpoint below. Current repair results and remaining P1D-008 are recorded in [the seven-repair successor](PULSAR_PORTABLE_PROJECT_P1D_REPAIR_STATUS.md). Original failure receipts are retained.

# Portable project P1d: first implementation, qualification blocked

## Status and authority

The Good phase and P1d are **incomplete**. This is an implementation checkpoint,
not a qualification or completion announcement. The canonical software
specification and architecture-first execution plan retain their authority.

Base commit: `4da4c08b59dc7b90e6c5aa3b420908257194eb8d`.
Current source/tool input fingerprint: `1d4b3f5bd08eba94bdd39fef80c14990d5672858e2624e7b8be800d66b684e75`
(210 declared entries; receipt records the exact method).
Production binary SHA-256: `4363e528526dce4b676b5dc40751fcfbf2e53efa71341e31e705a8889fe8b20d`.

No commit, push, physical actuation, GPU campaign, private-media ingestion,
native-window qualification, or change to the already running user preview was
performed in this pass. The newly written process-test target does not compile;
this is not a passing workspace gate.

## Implemented surface

- A Portable projects desktop tab invokes the shared production controller.
- Separate bounded control and transfer workers use persistent first-party
  clients, explicit packaging approval and project-scoped privacy consent.
- A non-secret session identity accessor allows lane-principal comparison;
  it does not authenticate a session or reveal its bearer credential.
- Export/import, status, local transfer stop, engine cancellation, explicit
  engine-copy release, public recovery IDs and explicit clone opening are wired.
- Off-tab polling/repainting keeps terminal replies consumable without forcing
  the ordinary preview worker to hash or transfer whole packages.
- Two new real-process scenarios and a qualification entry point were authored.
  These scenarios are **not executed evidence** because compilation failed.

The four confirmed presentation/recovery defects below remain in this first
implementation. Existing engine request fingerprints and authority checks
remain authoritative; the findings do not establish an engine bypass.

## Observed gates

| Command or campaign | Observed result | Boundary |
| --- | --- | --- |
| `cargo test -p pulsar-clients portable:: -- --nocapture` | Exit 0; 10 passed | Inline controller/policy tests, not real-engine or native-window evidence. |
| `cargo test -p pulsar-clients` | Exit 0; 76 passed; 0 doc tests | Includes the same 10 tests above; not 86 distinct tests. |
| `cargo build --bin pulsar` | Exit 0 | Production binary builds; existing engine warnings remain. |
| `cargo test --test portable_desktop_process -- --nocapture` | Exit 101; E0432 and E0277 | Zero new process tests executed. |
| `bash scripts/qualify-portable-desktop.sh` | Not run | Known uncompilable process target blocks the full chain. |
| Original historical 20-case campaign | Cargo 101 / runner 2; 0 of 20 executed | Preserved `serialport ^4.10` resolves to yanked 4.10.0 in the local offline index; no original lockfile preserved. |
| Required external review service | `review_diff` returned `Transport closed` | No external verdict. Independent source review is not a replacement. |

The previous 104 architecture checks, 517 Rust executions, 36 Node checks and
TLC/large-package results belong to base commit 4da4c08. They are not relabelled
as qualification of this new desktop implementation.

## Confirmed findings, not repaired

### P1D-001: Rejected export Start has no deliberate fresh-attempt path when no operation status exists.

Surface: production GUI. Evidence: source-confirmed; owner-confirmed; not dynamically reproduced.
Source: `crates/pulsar-clients/src/portable.rs:1012`.

Required repair: Preserve typed definitive-rejection versus unknown-outcome semantics and prior request/project/revision context. Offer an explicit fresh current-revision draft after a known rejection; never silently replace an ambiguous Start.

### P1D-002: PackagePrivacyConsent is imported from the client crate root, where it is not re-exported.

Surface: process-test compilation. Evidence: compiler-demonstrated E0432.
Source: `tests/portable_desktop_process.rs:6`.

Required repair: Import pulsar_clients::project_package::PackagePrivacyConsent.

### P1D-003: socket2::Socket cannot be converted directly into UnixStream by Into.

Surface: process-test compilation. Evidence: compiler-demonstrated E0277.
Source: `tests/portable_desktop_process.rs:649`.

Required repair: Transfer ownership safely through std::os::fd::OwnedFd, then UnixStream::from; no raw-FD or unsafe conversion.

### P1D-004: Cancelled ContinueImport assertion silently accepts unrelated successful reply variants.

Surface: process-test oracle. Evidence: source-confirmed; owner-confirmed; mutation not run.
Source: `tests/portable_desktop_process.rs:754`.

Required repair: Use an exhaustive match: validate noncompleted ImportStatus with no receipt, permit an interrupted-transfer error, reject every other successful variant. Remove the newly unused PackageOperationId import during the compile repair.

### P1D-005: A delayed nonterminal control snapshot can overwrite a completed import from the transfer lane.

Surface: production GUI. Evidence: source-confirmed; owner-confirmed; not dynamically reproduced.
Source: `crates/pulsar-clients/src/portable.rs:791`.

Required repair: Use an operation-bound reducer with terminal absorption and visible contradictory-receipt rejection. Preserve explicitly requested recovery of a different operation. Add reordered-lane regressions.

### P1D-006: Unrelated polling errors overwrite the only retained uncertain-publication recovery path and outcome.

Surface: production GUI. Evidence: source-confirmed; owner-confirmed; not dynamically reproduced.
Source: `crates/pulsar-clients/src/portable.rs:757`.

Required repair: Keep a separate bounded sticky recovery record bound to the original ticket, operation, Begin ID and destination until explicit user disposition; block further local publication while unresolved.

### P1D-007: An ambiguous import Start still permits Prepare to replace the retained source; Replay then uses the old request ID with the new declaration.

Surface: production GUI/controller recovery. Evidence: source-confirmed by owner and parent cached source; not dynamically reproduced.
Source: `crates/pulsar-clients/src/portable.rs:1247`.

Required repair: Pin the submitted declaration and retained-source context until the Start outcome is resolved or an explicit safe reset is authorized. Enforce the binding at the controller boundary and add an ambiguous-Start/source-replacement regression. The engine already rejects changed fingerprints; no engine authorization bypass is claimed.

The seven findings were preserved while confirmation to repair newly identified
defects is pending. Do not report the process scenarios as passed or P1d as
complete before repairs, independent regressions and the complete gate.

The independent review's source-ID-remapping concern was rejected after
confirmation from the planner owner: SourceVersionId is intentionally preserved;
project and candidate identities are fresh. No source-ID assertion was changed.

## Durable continuation

1. Repair the recorded findings after confirmation and retain original failure
   receipts. Add independent regressions for the four presentation/recovery bugs.
2. Compile and execute the two actual-engine controller scenarios. The stalled
   upload scenario must receive real engine acknowledgement before proving
   independent control progress; synthetic engine success is not evidence.
3. Run `scripts/qualify-portable-desktop.sh` with the existing large-package
   flags, recording exact current-source results. Keep native-window behavior
   and cross-platform qualification separate.
4. Retry external review without substituting an independent agent verdict.
5. Continue the 14 work packages in `PULSAR_GOOD_PHASE_CLOSURE_MATRIX.md`.
   Passing P1d cannot close the whole Good phase.

New science and historical records are in
`research/phase-b/runtime-portability-research-2026-09-12.md` and
`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/`.
The latter preserves exact source/test identities and the failed dependency
resolution, not reconstructed or passing original-test evidence.

Permitted real media/references, actual human review, identified reference
hardware and separately authorized device tests remain necessary. Software gaps
such as projected/mixed/live media, six-axis reconstruction, provider workers,
the local assistant and bundled offline installations remain engineering work,
not merely externally blocked evidence.

## Receipts

Machine-readable status: `evidence/phase-b-portable-project/p1d-checkpoint.json`.
Exact source inputs: `evidence/phase-b-portable-project/p1d-source.json`.
Raw focused, client/build and failed process logs are retained beside them.
