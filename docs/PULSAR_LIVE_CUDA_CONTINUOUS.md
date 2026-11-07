# Paced live CUDA ingress and handoff, 2026-10-09

Status: authored mechanism test PASS; continuous analysis handoff remains incomplete. Roadmap PARTIAL.

The [checkpoint](evidence/roadmap-2026-10-09/live-cuda-continuous/checkpoint.json) binds 409 source records: one new test and 408 unchanged. The release executable is unchanged. A producer sends 1,800 authored RGB frames over approximately 60 seconds at 30 fps, including model startup, explicit Continue and derived ContinueAt. Each epoch receives 600 frames over approximately 20 seconds; inference readiness is not a condition for sending frames.

| Epoch | Observed readiness | Uploads while Starting | Completed / accepted / stale | Pending replacements |
| --- | ---: | ---: | ---: | ---: |
| Initial Start | 9,011.90 ms | 270 | 331 / 331 / 0 | 269 |
| Explicit Continue | 9,019.44 ms | 270 | 331 / 331 / 0 | 269 |
| Derived ContinueAt | 8,943.53 ms | 268 | 333 / 333 / 0 | 267 |

The first result is observed at 9,046.32 ms. The maximum interval between distinct client-observed results is 9,052.40 ms, spanning a continuation. Those intervals include polling and are not a statistical latency qualification. They demonstrate that ongoing ingress alone does not eliminate the cold handoff gap. Current Continue retires the predecessor before the successor worker is ready; the test verifies Superseded status and preserved source identity. New frames coalesce in a bounded queue during startup. No stale results are accepted, but this is not lossless processing of all input frames.

Pending and in-flight queues remain at most one and drain empty. Observed probe-to-inference ages span 5.04–28.70 ms across 992 reports, a subset of completed results. All 1,800 headers match regenerated authored pixel hashes and advancing timestamps. Observed positions match the fixture formula within 9.536743161842054e-8. Only the last epoch consumes Finish and retains original evidence: 333 runs and 999 CUDA-only nodes, with SHA/length and capture-origin readback. Earlier epochs remain provisional. No offline jobs were created; owned processes are terminal and worker custody empty.

The initial test failed after two epochs because its ContinueAt timestamp was incorrectly wrapped in options. Public CLI rejected that shape. The corrected input is a raw numerator/denominator object. Failed source/logs and cleanup verification are retained separately. Current syntax, one native test without skips, 289 architecture checks and 36 guards pass. Prior 1,135 workspace/622 CUDA and stress gates are inherited on unchanged Rust source.

## Next implementation slice: prepared continuation

This slice is proposed and unimplemented. It must provide an explicit preparation mode while preserving existing immediate continuation behavior for existing callers.

1. Add a typed, opt-in continuation preparation declaration to public options. Reject its use on standalone Start. Preserve old canonical serialization when absent and fail closed for invalid declarations.
2. Keep the predecessor active while successor resources and worker prepare. Allow only one pending successor for that actor/project, count both against capacity, and preserve both original lease deadlines. Do not silently extend a lease or fall back to another backend.
3. Revalidate actor, grant, boot, project revision, source/clock correspondence, model/runtime identity and predecessor liveness before switching. Switch under Engine authority only after validated Ready. Retire the predecessor and reject later predecessor ingress/results without relabeling old observations.
4. Failed admission/preparation must preserve a still-valid predecessor. Insufficient aggregate RAM/CPU/GPU/storage capacity must refuse preparation rather than exceed owner limits. A predecessor stop, revocation, revision change or expiry must prevent later activation and retire dependent preparation.
5. Retain exact receipt/replay semantics without rearming on restart. The original final-trace contract and qualified/physical-admission boundaries remain separate. Preparation is not device authorization.
6. Verify default-mode compatibility, pending-successor capacity, stale/foreign Ready, failure preserving predecessor, predecessor stop/expiry, changed revision/grant, and restart/replay. Run authored paced CPU/CUDA cases with sufficient explicitly declared overlap capacity and a refusal case without it. Show continued predecessor output while preparing, atomic retirement at Ready, bounded queues, exact final evidence and confirmed worker/resource retirement. Do not infer model quality, six-axis reconstruction or physical-device behavior from those cases.

This probe excludes camera acquisition, encoded decoding, six-axis reconstruction and device dispatch. LIVE-02 and LIVE-09 remain unqualified. Prepared handoff, longer-duration work, useful model quality/real annotations, remaining desktop combinations, mixed clocks, installed Stash, platform/offline, physical Handy and human/release gates remain open. No training, live/sterile input, user service contact or physical playback occurred.
