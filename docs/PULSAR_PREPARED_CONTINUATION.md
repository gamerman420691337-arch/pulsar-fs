# Prepared live continuation implementation, 2026-10-09

Status: implementation and authored state tests PASS; workspace gate FAIL; native overlap UNKNOWN. Roadmap PARTIAL.

Public `LiveCaptureOptions.continuation_mode` accepts `immediate` (default, omitted from canonical serialization) or `when_ready`. Standalone Start rejects preparation; preparation requires inference and a Running predecessor. Explicit Continue can opt in, and ContinueAt inherits the predecessor's mode. Changing the mode does not change the declared source, model/profile or clock correspondence.

Preparation keeps the predecessor active and publishes the successor as Starting. Both count against the eight-capture limit and existing aggregate worker resource budgets; a second pending successor for the actor/project refuses. Neither original lease is extended. Frames can enter the successor's bounded queue while preparation occurs. Callers must continue delivering appropriate frames to the predecessor until the successor reports Running; this declaration does not itself acquire or duplicate a camera stream.

CPU Ready and validated CUDA Ready use the same authority-locked switch. Existing sweep checks current grants/revision/deadlines; switching additionally rechecks predecessor identity/owner/boot/clock, source/inference declarations, policy, cancellation, finishing state and both leases. CUDA worker/attempt/profile/manifest validation precedes the switch. A successful switch retires the predecessor as Superseded and marks the successor Running under the same lock. Prior ingress/results cannot acquire the successor's identity.

Failure retires the successor while preserving a valid predecessor. Stop, expiry, revision/grant loss or predecessor finalization cancels dependent preparation on the next sweep (normally within the existing 250 ms sweep interval) and before any Ready or ingress operation. Historical receipt/restart behavior continues to use the existing path; it is not new launch authority. Final CUDA trace publication remains tied to the original successor request and consuming Finish.

The owner policy used in previous scalar CUDA campaigns allows 128 MiB while each worker reserves 96 MiB. It cannot qualify overlapping workers. Positive native overlap requires an explicitly larger private authored policy; refusal with the existing policy must also be measured. No production owner limits, confinement or byte verification have been relaxed.

New authored state tests cover continued predecessor completion while preparation is pending, foreign Ready refusal, atomic Ready switching, preparation failure, predecessor stop/expiry/revision/grant loss, and second-successor refusal. A protocol test checks omitted-default roundtrip, invalid mode rejection and inference requirement. These synthetic state tests do not prove native worker overlap, resource admission or useful model output. Native paced CPU/CUDA overlap, insufficient-budget refusal, public receipt/restart cases, complete capacity cases and final evidence/retirement checks remain required before this slice is closed.

No real media, training, installed Stash contact or physical device actuation occurs in these checks. Camera/encoded acquisition, long-duration capture-to-command, six-axis output, useful quality/annotations, remaining desktop combinations, Stash, offline/platform/vendor, Handy and human/release gates remain open.

## Verification checkpoint

Final source passes 625 CUDA Engine tests (613 library and 12 integration; 68 ignored), 98 protocol library tests, 289 architecture checks and 36 source guards. All three new continuation state tests pass, including standalone Start refusal and full report validation after dependent retirement. These are state/contract tests, not native overlapped-worker qualification.

The workspace run returned FAIL: 1,129 passed, 10 failed, 63 ignored. Six package-import integration failures show startup timeouts or transport EAGAIN; four Engine import failures show import timeouts or a locked database. Diagnostics retain private QA paths. Root cause is not established. This run began before the final dependent-retirement error/report-validation correction; its failure must not be replaced by a later narrower green gate. Final CUDA/protocol verification uses the established private TMPDIR, while this workspace run used the inherited temporary directory. That difference is recorded, not proven causal.

The initial focused test build failed on private method access in new tests. A test-only Ready wrapper and the existing failure wrapper corrected it. Inspection then found that direct WorkerFailed retirement lacked the required error; dependent retirement now uses the existing bounded failure-report path, with explicit report validation in termination tests.

The source checkpoint compares 409 records to the prior continuous-ingress seal: ten changed paths, 399 unchanged. Existing release executable remains the prior source supplier and has not been rebuilt for prepared continuation. A new CUDA executable, positive native CPU/CUDA overlap, insufficient-budget refusal, public replay/restart, full capacity and final evidence/cleanup are the next required verification. Workspace fault diagnosis remains open.
