# Staging ownership lifecycle prototype

Status: test-only primitive implemented and exercised with actual owned-process termination. Engine integration, production journal format and database reader/downgrade protection remain pending. The existing engine crash regression still represents an unresolved production requirement.

The private Linux module is compiled only under `cfg(test)`. Its publication callback separates lifecycle effects from the future owning fence's durable storage. The file-journal adapter used by the crash tests is a bounded private fixture, not an adopted host format or portable representation.

Creation first binds a private canonical parent's identity and publishes intent. It checks that parent again after acknowledgement, creates the leaf through the held parent descriptor, synchronizes the parent, obtains the created identity and publishes that identity. A usable handle escapes only after acknowledgement and another locator check. A failed or lost acknowledgement never authorizes the next effect. Creation failure does not silently erase an uncertain obligation or delete an unbound resource.

Cleanup requires the caller to have independently established retirement of every byte user. It publishes a retirement record before recursive deletion, confirms deletion using the held directory and synchronizes the parent before clearing the record. Recovery refuses an existing resource with intent alone, a missing bound root, and changed parents or roots. An absent intent or retirement tombstone can clear only through its checked parent. Publication callbacks must synchronously commit the complete owning ledger; an in-memory callback does not provide durability.

The native fixture terminates and reaps its own child with SIGKILL at eight boundaries:

| Cut | Retained fixture state and observed recovery |
| --- | --- |
| Intent publication pending | No acknowledged obligation or created root. |
| Intent acknowledged | Intent remains; absent root reconciles and clears. |
| Identity publication pending after creation | Intent remains; existing unbound root is preserved and reconciliation refuses. |
| Identity acknowledged | Exact bound identity is recovered and cleaned after owner death. |
| Retirement publication pending | Bound identity and authored bytes remain; recovery checks and cleans them. |
| Retirement acknowledged | Retirement identity and authored bytes remain; checked cleanup completes. |
| Clear publication pending after deletion | Retirement tombstone remains; confirmed absence reconciles and clears. |
| Clear acknowledged | Root is absent and the fixture journal is already clear. |

Seven additional test functions cover publication order, lost intent/identity/retirement acknowledgements, deletion permission failure, missing bound roots, failed clear publication, root/parent replacement and replacement during acknowledgement. A new parent-handoff regression failed before the extra identity checks: creation returned a locator in a replacement parent. The original source and failure log are retained. The repaired case refuses before creation; a separate identity-acknowledgement replacement case preserves both the original directory and replacement canary.

The [evidence packet](evidence/roadmap-2026-10-07/staging-obligation/final-focused.json) retains the eight passing focused functions, all eight actual process-kill receipts, the preceding parent-handoff failure, final native identity, 182 production and 132 verification source records with archive readback, and 235 passing structural checks. The current feature suite passed 488 cases (20 ignored). The deliberately still-failing engine crash regression was explicitly rerun and exits 101; both results are recorded separately in `regression-gates.json`.

The next implementation must put these obligations in the engine's ownership scope, arm a reader requirement before creation, and integrate staging creation, worker transfer, native-image ownership, cleanup and startup. It must independently confirm prior workers/transport/capture users have retired before invoking recovery. The primitive itself does not establish that condition or retain resource reservations. Keep these synchronous filesystem operations in admitted preparation/cleanup/recovery work, with independent caller deadline observation. The complete creation/crash matrix, missing or downgraded records, portable correspondence, hostile same-UID races during recursive deletion, power loss and platform/model/media/Handy/human/release qualification remain open.
