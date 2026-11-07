# Persistent source placements

A source placement maps an immutable media source into project time. Multiple placements may reference the same source. The engine owns their durable definitions, active membership and history. Changing alignment leaves the neutral motion, authored action times and protection unchanged; the changed revision marks existing motion for alignment review. Generation remains a separate explicit action.

A placement declares a half-open trim in elapsed source seconds, a source anchor, a signed project anchor in nanoseconds and a nonzero rational source/project rate. Positive rates advance source time; negative rates reverse its mapping. The trim is declared input coverage, not a qualified duration measurement. The worker resolves actual source frames from its probed source index; it does not invent a frame at a trim boundary or retime original PTS.

Save a declaration such as:

```json
{
  "source_version": "SOURCE_KEY_FROM_PROJECT_SNAPSHOT",
  "source_start": {"numerator": 0, "denominator": 1},
  "source_end": {"numerator": 2, "denominator": 1},
  "source_anchor": {"numerator": 1, "denominator": 2},
  "project_anchor": 2000000000,
  "rate_numerator": 1,
  "rate_denominator": 1
}
```

```sh
pulsar project place PROJECT --spec placement.json
pulsar project preview-placement PROJECT PLACEMENT --time-ns 2000000000
pulsar project place PROJECT --spec changed.json --replace PLACEMENT
pulsar project remove-placement PROJECT PLACEMENT
```

The project snapshot supplies source and placement keys. Preview accepts the same optional `--video-view FILE.json` declaration as direct generation. CLI preview reports the immutable decoded-frame artifact and exact mapping receipt; it does not control a device.

`pulsar project generate-placement PROJECT PLACEMENT` submits a placed, model-free candidate generation job without committing existing motion. Desktop generation uses the selected placement with its normal model settings. The engine requires exact active-definition/source correspondence; mapped negative motion coverage or colliding nanosecond samples refuse. Exact source mapping, original PTS and project sample order are retained, including reverse playback. Unsampled trim boundaries remain unavailable gaps. See [ADR B4-005](../research/phase-b/ADR-B4-005-placed-generation-sampling.md).

Desktop's project panel exposes **Source placements**, trim/anchor fields in seconds and a signed speed fraction. Add or replace creates an immutable mapping; each replacement gets a new placement ID. Selecting it changes preview to project time. Original clip-clock preview remains available. GUI time fields resolve to declared nanoseconds; exact mapping and source-frame selection use rational/integer arithmetic. The shared API and CLI paths are exercised by native process tests; interactive GUI usability is not qualified.

Placed preview results bind the resolved definition and requested project time to the actual source frame, original PTS, source origin, elapsed position and exact actual project time. Exact fractions use decimal-string numerator/denominator fields so control consumers do not lose large integers. A separate nanosecond quantization report retains rounding remainder. The source-frame policy is the predecessor in source time; reverse mapping can therefore report an actual project time after the request. This is the actual sampled frame, not a fabricated project-clock match.

The engine checks current placement membership before and after native execution. Clients reject another definition, source, placement, transform, seek/request generation or inconsistent mapping/quantization receipt. A sampled predecessor outside the declared trim is refused rather than presented as trim coverage. Requests outside the mapped trim are refused.

Set/remove commands require project Edit authority, expected revision and durable request identity. Replay returns the original outcome without another definition or revision. Undo/redo restore placement membership together with motion/protection history. Removed/replaced definitions remain immutable history; source snapshots referenced by placement definitions are retained for recovery. Alignment warnings do not silently disappear when another candidate is committed; durable review-resolution behavior remains an independent obligation.

Placement-aware persistence began at database schema 3. Schema 4 added source-time publication proof references; current schema 5 additionally preserves explicit review-disposition semantics. Supported schema-2/3/4 databases migrate transactionally while preserving existing rows; incomplete current placement/proof layouts require explicit recovery. The retained schema-2 and schema-3 engines must refuse the current database. These are tested executable subjects, not claims about every historical binary.

Portable project format 3 retains all immutable definitions, active membership and revision/edit-history membership, including removed placements. Clone import assigns fresh placement IDs while preserving alignment, source content identities, revision ordinals and unchanged motion. Explicit original namespace mappings survive successive re-export and reject altered historical alignment or contradictory ancestry. Formats 1 and 2 retain their canonical bytes; the pinned older reader refuses format 3 before project publication. See [ADR B4-004](../research/phase-b/ADR-B4-004-portable-placement-closure.md). Explicit existing-motion retiming, portable Stash source/proof association migration, broader encoding/platform coverage, real projected-media quality, synchronized audio and physical playback remain open. This increment does not close GB-03, Phase B or the roadmap.

The checked source-clock artifact transform preserves original sample clocks and maps other authored timestamps explicitly without editing the master. Its native decode/generation correspondence is exercised; the bounded engine receipt loader now verifies original candidate/attempt/configuration/placement custody. Explicit `stash publish-script --source-time` now maps a placed descendant without rewriting its master and retains a private timing proof with the pending publication. Desktop offers the same explicit checkbox. Successful replay and restart are covered locally; query-only `stash publication-status` now inspects uncertain dispatch/proof/source/script state without redispatch or promotion. Explicit uncertain-publication disposition and portable Stash source/proof migration remain open. Schema 4 introduced the durable proof-reference boundary; schema 5 adds explicit review-decision semantics. See [ADR B4-006](../research/phase-b/ADR-B4-006-source-clock-artifact-mapping.md).
