# Model-pack approval withdrawal

The owner can now withdraw an exact model-pack approval and cancel unfinished generation admitted under it. CLI and desktop controls use retained original request IDs; [ADR B3-081](../research/phase-b/ADR-B3-081-model-pack-approval-withdrawal.md) records the authority and compatibility decisions.

```sh
pulsar model pack-withdraw MANIFEST_SHA256 --selection-revision APPROVAL_REVISION --expected-selection-revision CURRENT_REVISION --request-id RETAINED_ID
```

Refresh current selection before preparing withdrawal. Withdrawing the current approval deactivates it and advances the selection revision. Withdrawing a historically issued approval preserves a newer selection and its generation. Direct generation remains independent even if its artifact references equal the pack dependencies.

New generation has an atomic private admission record identifying direct or pack origin. Older unfinished generation without that record refuses withdrawal by default. Cancel those jobs individually, or explicitly add `--include-unclassified-generation`; this broader scope cancels all unclassified unfinished generation, including potentially direct legacy jobs. The desktop has an explicit checkbox for that scope. Corrupt records and more than 1024 pending jobs refuse before changes. Missing or expired pack assets do not prevent metadata withdrawal.

Cancellation state and replay receipt commit together before captured worker handles are signalled. Failed receipt writes or COMMIT leave selection, jobs and signals unchanged. Replay does not cancel replacement workers. A cancellation receipt proves committed intent; worker retirement, resource release and actual native/device effects remain separate. Restart/resume cannot publish a cancelled job.

The private admission namespace is outside the checked client SessionId alphabet. Receipt hashing detects accidental corruption, not replacement by the host database owner. There is no new table or portable format; the client recovery journal uses schema 3 for withdrawal and refuses a downgraded reader barrier. It stores intent and credential binding, never the owner proof. Receipt hydration checks current versus historical revision behavior and exact request scope before acknowledging recovery.

Final all-feature engine library passed 723 tests with 79 ignored; the protocol library passed 109 and frozen-wire checks passed. Seven pack-recovery tests passed both on tmpfs and alone on ordinary storage; seven headless panel tests passed. The complete client library failed with 196 passed, one concurrent journal busy refusal and four ignored. An earlier engine run also failed three storage/producer-hook cases. Later passing runs do not prove those defects repaired. Formatting, strict workspace default/all-feature Clippy, architecture 352 and guards 36 passed.

Verification and all failures are retained in [the checkpoint](evidence/roadmap-2026-10-10/model-pack-withdrawal/checkpoint.json). This workflow covers offline generation only. Automatic catalog-triggered cancellation, live capture, Preview and native preparation withdrawal, current storage/lifecycle failures, real model/media/platform/rendered/Handy/human/release acceptance remain open. Full roadmap INCOMPLETE. Sterile references remain evaluation-only; no annotations or useful model-training quality are claimed.
