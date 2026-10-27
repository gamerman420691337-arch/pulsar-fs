# Observed stop completion metadata reconciliation — 2026-10-10

The conditional Handy coordinator now retains a checked callback observation when its durable outcome write fails. `complete_stop_with_recovery` validates the current sealed stop and exact receipt, retires host stop authority, captures the original effect/receipt/outcome and attempts the journal write. Its returned `StopCompletionRecord` exposes the observed outcome and `journal_pending`; callers must retain a pending record and explicitly retry `persist`.

The record has no public constructor, mutation of captured facts, request accessor, cloning or deserialization. A retry can only update its original receipt with its captured outcome under the existing boot-owner, content identity, stored integrity and terminal-outcome checks. It does not issue or replay a transport effect. Pending is an incomplete metadata write, not durable success. A failed attempted persistence conservatively leaves the pending flag set.

Successful historical recording after a newer stop or reconnect cannot change runtime authority, retire that newer token, rearm playback or clear physical uncertainty. A newer engine boot fences the record. If the process loses an unrecorded observation, the saved prepared row stays unknown; it cannot reconstruct fresh observed facts or stopping proof. Full restart/outcome reconciliation and retention remain open.

Existing `complete_stop_durable` delegates to this path and retains its earlier success/JournalUnavailable result behavior. Existing stop token traits, journal schema/encoding/capacity, protocol, project/portable formats and physical capability discovery remain unchanged. Callers needing explicit metadata recovery opt into the additive interface. See [ADR B5-019](../research/phase-b/ADR-B5-019-observed-stop-metadata-reconciliation.md).

Current local evidence on dirty branch `codex/pulsar-roadmap-m1`, base `4da4c08b59dc7b90e6c5aa3b420908257194eb8d`:

- All-feature engine test build PASS. Coordinator **32 PASS**, zero ignored, 5.88 seconds; four new cases exercise both outcome values under an actual private SQLite UPDATE-abort trigger, same-receipt/idempotent retry, newer-stop noninterference, cross-boot refusal, reconnect uncertainty and receipt substitution. Existing legacy completion-failure tests remain passing.
- Complete all-feature engine library **716 PASS**, zero failures, **79 ignored**, 19.07 seconds. Intermittent earlier staging/export/native failures are not asserted repaired by this passing run.
- Default and all-feature workspace/all-target strict Clippy PASS. Formatting PASS; architecture 350 and source guards 36 PASS.
- The unchanged revised client pack verification remains a **non-pass**, exit 137 after the original 45-second command limit and kill grace, with no passing summary. Its current binary hash matches the preceding journal-contention checkpoint. No new client or workspace runtime PASS is inferred.

[Public checkpoint](evidence/roadmap-2026-10-10/stop-completion-reconciliation/checkpoint.json) retains exact source/binary identities, original coordinator source and public gate logs. Only `playback_runtime.rs` changes from the previous client-contention source checkpoint; 347 source identities remain unchanged. Private test state is excluded and removed after owned references disappear.

Physical playback still reports false; public Arm/Stop/SwitchPlaybackRevision stay unsupported. No actual Handy was contacted. The registry/evidence/authority/dispatcher, emergency stop behavior, full historical reconciliation/retention, process-death/power-loss and platform coverage, actual Handy 2 Pro firmware/sync/stop, six-axis physical acceptance, models/media/annotations, offline assistant/distribution, rendered GUI, human and release acceptance remain open. Full architecture-first goal remains active and incomplete.
