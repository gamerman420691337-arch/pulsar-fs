# Assistant retirement diagnosis

2026-10-12. **Eight explicit native tests PASS; original learned-model retirement failure remains UNKNOWN/unreproduced.** No production change, deadline extension, weakened custody, or model/default selection is made. Full roadmap remains PARTIAL.

The preceding [instruction-candidate campaign](PULSAR_INSTRUCT_MODEL_CANDIDATE.md) retained one actual Engine/CLI unavailable-yaw request that failed without a proposal with `worker tree retirement unconfirmed`. Its instrumented rerun did not repeat that error and instead completed with an incorrect stroke hold. Parent-process joining and a later empty ledger do not retroactively close the original failure. Those two outcomes stay separate.

The current parent/custody feedback commands use the existing compiled Engine library test executable with actual delegated Linux cgroups. A network-disabled bubblewrap wrapper provides an owned RAM-backed temporary root and writable delegated cgroup mount. No learned model, media, user library, training annotation or physical device is involved. The exact source and executable identities are retained in the [packet](evidence/roadmap-2026-10-12/assistant-retirement/checkpoint.json).

| Executed existing test scope | Native result | What it establishes |
| --- | --- | --- |
| Assistant parent transport/lifetime test | 1 test PASS, five cases, 2.22 seconds | Correct response, stale identity, excess output, deadline and descendant paths use captured assets and retire owned custody/admission. The original learned trigger is not reproduced. |
| Worker-tree retirement tests | 7 tests PASS, 0.40 seconds | Actual orphan signalling; held transport thread; failed staging deletion; blocked durable roster clear; failed registration/spawn/pre-exec. Tests require held scope/staging/admission until the obstruction clears and confirmed later retirement. |

The blocked-thread test explicitly receives TimedOut from immediate retirement after direct-child exit, preserves its staged bytes, storage claim and exclusive scope lease, then confirms release only after the retained thread ends and cleanup completes. The staging-permission fault similarly retains the complete obligation until permission is restored. These are intentional custody-failure controls; they do not reproduce the causal conditions of the earlier learned request or prove its root cause.

The parent may return a successful proposal only after full tree/fence retirement within its unchanged deadline. The common custody reaper retains assets and admission when synchronous retirement cannot be confirmed. This conservative behavior is required by [ADR B3-084](../research/phase-b/ADR-B3-084-engine-owned-assistant-execution.md). No result here justifies converting unconfirmed cleanup into success or releasing capacity early.

Before and after the probes, the read-only cgroup inventory contains the same 81 pre-existing paths with the same device/inode identities. No added or removed path remains. The test fixtures reclaim their own resources; unrelated empty groups are untouched. These snapshots verify this probe's observed resource effects, not historical ownership or all prior cleanup.

The selected eight tests were ignored in the ordinary default suite and are explicitly executed serially here. They are not a fresh complete library/all-feature/platform suite. Production Rust, App/Worker and test binary remain unchanged. Current architecture/source-guard results are recorded separately. Temporary wrapper/output scratch is removed after terminal handles, source/binary validation and accessible process-reference inspection; the document and evidence packet remain. The original failure still needs a tight reproducible learned trigger before a corrective implementation is justified. Other roadmap/model/media/device/platform/human gates remain open.
