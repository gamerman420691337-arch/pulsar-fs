# Retained live CUDA evidence inspection

Use `pulsar live-capture evidence PROJECT CURRENT_REVISION CAPTURE_EPOCH` to inspect retained historical CUDA descriptors. The capture need not remain in the live registry. The command also works after Engine restart with a CPU-only build and no CUDA owner policy.

The Engine requires current project Read authority and the project's current expected revision. The response preserves the original capture identity/revision, boot, actor session, Start request and verified evidence descriptors. An unknown, cancelled or foreign-project epoch returns NotFound; stale expected revisions and revoked/missing Read authority refuse. Historical consistency does not restore capture authority, resolve review, qualify motion or permit physical effects.

Inspection shares the startup store's per-row reader: SQL shape/length/aggregate bounds precede original blob loading; a 512 MiB memory reservation covers evidence decoding; canonical origin/manifest/summary, indexed scope, original admission membership and receipt/trace correspondence are rechecked. The query returns bounded descriptors, not original blob contents or paths. It neither loads an inference runtime nor opens recorded worker, media or library paths. Database and worker schemas remain unchanged; the command and response are additive protocol vocabulary. Memory admission failure also refuses inspection.

An actual CPU-only shipped CLI reopens the preceding authored CUDA run, retrieves descriptors identical to its original completed report, refuses cancelled/foreign epochs and stale revision, and repeats the same inspection. Independent read-only SQLite readback confirms all five original blobs are unchanged, one retained row, no offline jobs/imported sources and no foreign-key errors. Focused tests cover a later current project revision, Read revocation, missing epochs, corruption, oversized SQL and memory pressure. Two compile-time development mistakes are retained in the packet; neither represents executed passing tests.

Fresh gates pass 1,036 serial workspace tests (55 ignored), 552 serial CUDA Engine tests (63 ignored), the default build, the native CLI case without skips and 266 structural checks. All 365 live/snapshot/archive source records match.

See [ADR B3-067](../research/phase-b/ADR-B3-067-retained-live-cuda-inspection.md) and [the checkpoint](evidence/roadmap-2026-10-08/live-cuda-evidence-inspection/checkpoint.json) for the tested subject and gates.

Original-byte download/export, listing/retention controls and desktop presentation remain open. This adds historical inspection, not roadmap or release completion. Additional capture authority/deadline/death/overlap cuts, sustained acquisition, known parallel failures, model/media/multimodal/six-axis/performance/Stash product work and Handy/platform/human/release qualification remain open. The preceding native startup timeout remains unresolved.
