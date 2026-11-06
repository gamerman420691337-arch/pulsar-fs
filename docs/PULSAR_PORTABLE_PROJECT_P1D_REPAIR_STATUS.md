> Successor: [P1D-008 is now locally repaired](PULSAR_P1D008_REPAIR.md). Its external review remains unavailable. Historical results below remain bound to their original source.

# Portable project P1d: seven-repair checkpoint

Date: 2026-09-14
Status: **Seven approved repairs implemented and locally exercised. P1d and the Good phase remain incomplete.**

## Authority and identity

This successor updates the current interpretation of [the historical first implementation](PULSAR_PORTABLE_PROJECT_P1D_STATUS.md); its failed receipts remain unchanged. The software specification and architecture-first execution plan retain authority.

Last identified base: `4da4c08b59dc7b90e6c5aa3b420908257194eb8d`. No commit or push was made in this repair pass.
Source/tool input fingerprint: `7f0b4d1440a4592be8488d9d78b82494db7a46c83a7f48f605616fadf7bc2560` (210 declared entries; same method and input list as the first checkpoint).
Only `crates/pulsar-clients/src/portable.rs` and `tests/portable_desktop_process.rs` differ from that checkpoint's declared inputs.
Production binary SHA-256: `751dc80baf04e9a0cbc279f8be6c1f8f5f414835a1408f8b3befebf09e368e87` (403258464 bytes).

## Observed qualification

| Campaign | Result | Evidence boundary |
| --- | --- | --- |
| Valid RED controller regressions | Exit 101; 1 passed, 5 intended failures | Six tests reached the intended paths. The source fixture uses a valid package header, not arbitrary bytes. |
| First GREEN attempt | Exit 101; E0369, no tests executed | Explicit terminal-field comparison subsequently replaced unsupported whole-status equality. The failed source and log are retained. |
| Final focused regressions | Exit 0; 7 passed | Includes an added unrelated-import recovery/retained-source regression. The seventh test was not part of the six-test RED campaign. |
| First full-gate attempt | Interrupted during compilation; no final exit observed | Not a pass. Its partial log remains separate. |
| Full portable-desktop gate, both large-package flags enabled | Exit 0; 559 passing Rust test executions | Repeated suites are included in this count; it is not 559 unique tests. Workspace clients: 83 passed. Focused portable suite: 17 passed. |
| Actual-engine portable process target | 4 passed in each of two gate invocations | Two real-engine scenarios plus two cancellation-oracle helper tests; not four unique real-engine scenarios. |
| Architecture/source scanner | 106 structural checks and 36 scanner tests passed | Static checks do not prove runtime confinement or all platforms. |
| Lifecycle/model checks | Gate passed, including expected negative mutants | Lifecycle baseline: 273 distinct states; authorization-negative mutant violated NoUnauthorizedCommit as required. Motion-transfer baseline: 852 distinct states. Package export/import model checks and large-data campaigns completed through the gate. |
| External repair diff review | Primary **PASS WITH NITS** | Critic output was truncated mid-finding; no complete critic/ensemble verdict is claimed. This is not review_commit of the base or a future commit. |

Command:
```sh
PULSAR_RUN_LARGE_PACKAGE_TEST=1 PULSAR_RUN_LARGE_PACKAGE_IMPORT_TEST=1 \
  bash scripts/qualify-portable-desktop.sh
```

Runs used the persistent repository target, two Cargo jobs, incremental compilation disabled, offline Cargo, and the existing pinned TLC jar SHA-256 `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`. Existing compiler warnings remain; this is not a warning-free claim.

The process harness relocates generated source bytes, makes the origin unavailable, imports into unrelated roots, edits the cloned project, re-exports and imports again, and checks exact source identity/content and motion. SourceVersionId is deliberately preserved; project/candidate identities are fresh. Its cancellation test forwards a real engine upload ACK, holds it, and proves independent status/cancellation before releasing it. Injected controller status snapshots and header-only prepare fixtures are separate component evidence, not real detector or real-media qualification.

## Repair dispositions

| Finding | Implemented behavior / regression |
| --- | --- |
| P1D-001 | Typed conservative rejection classification enables an explicit fresh current-revision export draft, preserves one prior context with explicit disposition, and clears consent. An earlier ambiguous Start remains pinned after a later rejection. |
| P1D-002 | Import PackagePrivacyConsent from its actual project_package module. Process target compiles. |
| P1D-003 | Transfer socket ownership safely through OwnedFd before UnixStream conversion. No unsafe/raw-FD conversion. |
| P1D-004 | Exhaustive cancellation helper rejects unrelated successful replies; actual helper negative and permitted-status tests pass. This is not a comprehensive mutation campaign. |
| P1D-005 | Same-operation terminal import state absorbs late nonterminal replies; contradictory terminal identity/receipt becomes a visible failure while preserving the first receipt. |
| P1D-006 | Controller retains one ticket-scoped publication recovery record independently of dismissible/transient errors. New publication stays blocked until explicit matching-ticket disposition. No file deletion or engine-copy release is implied. |
| P1D-007 | Controller binds retained source/declaration to the original Start identity. A bounded restorable view survives unrelated recovery, and unrelated Continue cannot consume the original handle. Matching terminal resolution is required to clear the pin. |

Independent source review found no remaining confirmed issue within those seven repairs. The external review's terminal-ordering question was checked against final source: `same_import_terminal_binding` and the visible contradiction error occur **before** `continue`. That question was a false positive; no change was made. Optional readability/style nits were not applied.

## Newly recorded, not repaired: P1D-008

**P2, source-confirmed, pre-existing; repair approval pending.**

An ambiguous export Start A can lose its visible replay identity when the user queries another export B. The Import-side repair does not fix this separate Export-side path: `portable.rs:987` replaces the export request/target with B; the fresh-draft path at `portable.rs:1475` can then preserve B rather than A. Preserve A's unresolved identity in a bounded recovery surface before replacing the current view.

No new dynamic reproduction is claimed for P1D-008, and no engine authorization or request-fingerprint bypass is established. It prevents declaring P1d complete despite the passing gate.

## Remaining scope

No native-window, cross-platform, physical-device, GPU performance, private-media or neural-accuracy qualification occurred. No running user preview was replaced. The original historical 20-case campaign remains 0/20 executed because its preserved offline dependency resolution is blocked; this gate does not substitute for it.

The [Good-phase closure matrix](PULSAR_GOOD_PHASE_CLOSURE_MATRIX.md) retains the other software, benchmark, modality, runtime, assistant, device and distribution obligations. Passing this repair gate does not complete B1-B6 or authorize the phase-completion keyword.

## Durable evidence

[Repair receipt](evidence/phase-b-portable-project/p1d-repair-2026-09-14/receipt.json), [source manifest](evidence/phase-b-portable-project/p1d-repair-2026-09-14/source.json), [review disposition](evidence/phase-b-portable-project/p1d-repair-2026-09-14/external-review.json), and [full passing log](evidence/phase-b-portable-project/p1d-repair-2026-09-14/full-gate-resumed.log).
Original RED source, failed first GREEN source, sequential repair patches, RED/failed/green logs and the interrupted full-gate log are retained alongside these files.
