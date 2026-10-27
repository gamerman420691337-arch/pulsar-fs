# ADR-B3-114: scope ambiguous association to affected identities

Status: implemented software; native verification recorded separately; not qualified.

## Evidence and decision

The known upward patch-motion fixture retains a continuously observed class-5 target, but its final model-sized frame also has ambiguous unrelated detection associations. The worker previously blocked the entire frame whenever the tracker emitted its global ambiguity review. This withholds valid selected-target motion because of other identities. The predecessor native receipt and export refusal are retained.

Keep the global review and expose a checked per-update ambiguity query over affected identity IDs. Capture affected prior identities before aging/removal; mark fresh identities created from ambiguous detections as affected as well. This set is produced only by the bounded tracker and cannot be supplied by a client. It lasts for one update. Ordinary class-aware association, ambiguity margin, missing-observation handling, cuts, context changes and point/motion admission stay unchanged.

The worker blocks motion for an affected selected identity. An independently associated observed selected identity can continue, retaining the global `ambiguous_tracking_association` review. An affected selection also retains `ambiguous_target_association`. Fresh identities born from ambiguous detections cannot bypass the block. Missing observations and identity discontinuities retain their existing guards.

Detector generation recipe v4 identifies this behavioral change. The checked receipt reader accepts historical v2/v3 and current v4 only for the detector mode, preserving remaining custody checks. No historical receipt is rewritten. See [native evidence and limitations](../../docs/PULSAR_RELATIVE_MOTION_EVIDENCE.md).
