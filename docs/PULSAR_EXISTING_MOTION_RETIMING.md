# Explicit existing-motion retiming

Changing a source placement still leaves motion unchanged. The separate production CLI operation makes an intentional clock change:

```text
pulsar project retime PROJECT OLD_PLACEMENT NEW_PLACEMENT \
  --axes stroke,sway --revision CURRENT_REVISION --request-id RETAINED_REQUEST_ID
```

The desktop Source placements panel exposes the same scoped operation with an old placement ID, selected new placement and explicit available-axis checkboxes. It does not calculate or authorize the retime locally.

The authenticated engine requires Edit authority and the exact current revision. The old immutable placement must be retained in this project; the new placement must be active in this project. Both must reference the same source version. This permits retiming after replacing a placement without requiring the old placement to remain active. Explicit axes must be unique and present. Other tracks and placement membership remain unchanged.

Each selected action maps from its old project time to exact rational elapsed-source time, then to the new placement's project time. Positions and evidence labels remain unchanged. Reverse mappings reorder actions; they never invert positions. Project action timestamps floor to nanoseconds. Negative times, arithmetic overflow, equal quantized timestamps, unrelated sources and actions outside either trim refuse the entire edit. No clipping, dropped action, interpolated trim anchor or automatic timeline shift is performed. New trims that still contain all selected action/gap support are supported; destructive trimming and reconstruction of boundary anchors remain separate design/implementation work.

Unknown gaps map with exact boundary arithmetic and outward nanosecond bounds. Reversal includes the formerly closed endpoint conservatively. If rounding creates overlapping gaps or an action inside an unknown gap, the entire edit refuses. This does not resolve uncertainty or assert physical motion across a gap. Conservative reversal can therefore refuse some otherwise mathematically mappable boundary layouts.

Protected spans use the existing complete-segment guard, including neighboring interpolation anchors. Failed validation commits no revision. A successful edit records immutable before/after motion, authenticated actor, revision/history and retained placement IDs in its revision description. New schema-6 edits also commit a [structured retiming receipt](PULSAR_MOTION_RETIME_RECEIPTS.md), whose status command verifies those bindings and recomputes the transformation. Undo/redo and restart retain the resulting program; replay of the same request returns the original snapshot. Changing its intent or using a new stale-revision request is refused.

Original review evidence remains unchanged in its original clock. Existing review decisions retire; a new review-required marker covers the changed program, and provenance remains explicitly conservative/unknown. This is an authored time transformation, not renewed model evidence or human acceptance. [Retiming-aware Stash source-time publication](PULSAR_RETIMED_STASH_PUBLICATION.md) now verifies the structured receipt chain and carries exact origin sample clocks through the historical edits. Explicit source-time intent is required; missing receipts and mixed stroke clock histories refuse. Portable archives can retain the revision as declared historical lineage without granting it executable authority.

Pure tests cover forward/reverse timing, fractional quantization, preserved values/evidence, selected-axis isolation, gaps, collisions, trims, sources and protection. Authenticated-engine tests cover authority, revision conflict, exact replay, protected refusal, history and restart. Native CLI validation uses authored script values and an inert source file that is never decoded; it is not a real-media synchronization or physical-device test. Exact final gate results are recorded in the roadmap evidence directory. Broader source/seek, destructive trim editing, source-clock proof migration, manual desktop usability qualification, real-media/model comparison and device qualification remain open.
