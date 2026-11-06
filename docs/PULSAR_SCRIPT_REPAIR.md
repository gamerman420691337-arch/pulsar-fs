# Standalone script repair

The CLI fix command now sanitizes and numerically repairs a standard funscript through shared Engine authority.

```sh
pulsar fix original.funscript
pulsar fix original.funscript --output repaired.funscript --max-speed 300 --jitter-window 20 --jitter-threshold 2
```

Default output is `<stem>.fixed.funscript`. Existing destinations, including the input itself and aliases, are refused. Engine publication is also atomic and no-clobber. The raw source file remains unchanged. Invalid policy/destination checks precede project creation; source/transport/edit/export failures retain the named project for inspection. There is no automatic replay or rollback.

Explicit repair import stably sorts timestamps, clamps signed integer positions to 0–100, and keeps the last input position at duplicate timestamps. It records an immutable raw-source identity and synthesized sanitation review. Ordinary import remains strict. Negative or overflowing timestamps, malformed JSON, empty input and excessive action counts refuse; repair does not invent time or support. Inverted input follows the existing neutral import convention.

Core removes interior actions when the time since the last retained point is strictly below jitter window and position change is at most jitter threshold. First/last timestamps remain. It then clamps integer-percent deltas to the floor of maximum percent/second times elapsed milliseconds/1000. Jitter window must be nonnegative and representable; jitter threshold is 0–100; maximum slope must be finite and within the existing checked segment-speed domain. Endpoint positions can change. This is an average numerical script-slope policy, not motor safety, acceleration/peak-velocity admission or motion quality.

The retained project has a sanitized imported baseline and a separate authored-values repair revision. Existing upload receipts, source ancestry, candidate/revision guards and protection checks remain authoritative. Undo restores the sanitized baseline; Redo restores the repair. Undo does not rewrite the raw file or reconstruct invalid positions. Generated values are untrusted at upload and never grant observation or device authority.

[ADR B3-141](../research/phase-b/ADR-B3-141-shared-script-repair.md) records the architecture. The [script-repair checkpoint](evidence/roadmap-2026-10-12/script-repair/checkpoint.json) records four Core controls, two Engine sanitation/replay/clock regressions, one Client destination/alias test and actual native checks. The predecessor refuses fix. The rebuilt network-disabled CLI passes three independent repaired-output oracles, raw-source/edit-lineage checks, actual Undo/Redo and diagnostics, and nine refusals. Six policy/destination refusals leave project count unchanged. Both predecessor/final Engines join. Full selected Engine/Clients suites pass 841/310 with 70/8 ignored; Core/Protocol default suites, strict lint in both configurations, workspace check, formatting, native App/Worker/controller build, 441 architecture checks and 36 source guards pass.

The first native history probe mistakenly included a punctuation period in its project ID and was correctly refused; that failure is retained. Correcting the probe passes without changing production authorization. [Desktop repair controls](PULSAR_DESKTOP_SCRIPT_REPAIR.md) are now implemented as uncommitted proposals. Native rendered GUI, broader storage/process failures and all model/media/device/platform/human/release qualification remain open.
