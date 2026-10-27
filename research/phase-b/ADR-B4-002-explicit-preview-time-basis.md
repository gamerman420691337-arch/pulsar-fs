# ADR B4-002: Explicit preview request time basis

Status: implemented timestamp contract; source-placement and broader qualification gates remain open.

## Problem and reproduced seam

Desktop's playhead is elapsed clip time. The earlier worker compared that value directly against absolute source PTS and used `partition_point(...).saturating_sub(1)`. For a first source timestamp of 5 seconds, a 0.300-second desktop seek therefore chose frame 0 instead of the actual predecessor of 5.300 seconds. Generation already subtracts the first source PTS. The same apparent clip position had different preview and generation origins.

The baseline behavior is established from the prior worker selection code and its deterministic nonzero-PTS input; a historical binary replay is not claimed. The new native fixture carries a 5-second source origin and irregular successive PTS. Its eye/frame channel independently identifies which source frame was decoded.

## Decision

Use one pure checked source-frame seek kernel, with an explicit `PreviewTimeBasis`. Keep `SourcePts` as the legacy omitted serialization default. Add `ElapsedFromFirstFrame` and use it in the desktop's production request path. The worker uses its probed immutable source index as the origin; clients cannot supply a fabricated source origin. The engine carries the basis into the confined worker and validates the returned request association.

Retain exact requested/actual/source-origin/elapsed rational timestamps and the actual frame ordinal. Match request clock, frame/evidence context, source/placement/transform and seek/request generations. Reject inconsistent origin/elapsed arithmetic, actual frames after the requested time, negative elapsed input, invalid source indexes and checked arithmetic overflow. Before-first-frame requests refuse instead of pretending a timestamp was available. Requests beyond the last known PTS hold and report that actual frame without claiming a duration.

## Frozen verification criteria

- Source origin 5 seconds; elapsed 0.300 resolves ordinal 1 at elapsed 0.125, elapsed 0.375 resolves ordinal 2, absolute 5.300 resolves ordinal 1.
- Native selected-right-eye fixture pixels retain the independent `100 + ordinal` channel.
- Authenticated engine and shared desktop API resolve elapsed 0.500 and absolute 5.500 to ordinal 6 with identical decoded pixel hashes; restart preserves that resolution.
- Core tests cover negative and positive origins, irregular indexes, empty/repeated/reversed PTS, before-origin refusal, last-frame hold and arithmetic overflow.
- Client tests reject clock changes, inconsistent origin, a future actual frame, missing elapsed metadata and late results from another placement.
- Legacy request serialization omits the default basis and deserializes it as `SourcePts`; explicit negative elapsed requests fail before native execution.

## Remaining obligations

This primary clip-relative mapping is not a persistent project timeline placement implementation. Edited placement/trim/rate/offset descriptors, placement migration, full seek/encoding/platform coverage, real projected-media/model comparisons and audio/device timing remain open. No model, release or physical-device qualification follows from this timestamp repair.
