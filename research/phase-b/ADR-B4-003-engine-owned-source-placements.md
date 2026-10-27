# ADR B4-003: Engine-owned immutable source placements

Status: implemented local persistence/preview contract and versioned package closure; placed generation is implemented by ADR B4-005; broader qualification remains open.

## Contract and decision

ARCH CONTRACT-02 requires explicit rational source placement transforms, offsets/rate changes, immutable source versions, unchanged existing motion on alignment edits and stale derived-result review. CONTRACT-03 requires matching source/placement/transform/frame and request generations. Implement these contracts through checked core values, engine transactions and the shared preview worker; clients remain presentation only.

A `SourcePlacementSpec` declares elapsed-source trim/anchor, signed project anchor and signed rational source/project rate. `ExactSeconds` normalizes checked i128/u64 fractions, performs checked rational composition and comparison, and serializes integers as decimal strings. Mapping remains exact beyond the older source-timestamp denominator range; nanosecond quantization is a separately retained report.

Each definition receives a new engine placement ID and creation revision. Replacement removes the prior active membership while preserving the immutable definition. Active, revision and edit-history membership are persisted separately. Alignment commits preserve motion/protection bytes and add conservative alignment review. Undo/redo restores membership with the corresponding edit state. Current membership is checked again after a worker returns. Referenced source snapshots remain retained for recovery.

Preview commands carry a placement key and project time. The engine resolves its definition, validates context/source authority, and admits one shared worker Preview operation with a typed placed seek. The legacy timestamp fields have a fixed zero/elapsed compatibility shape in that internal manifest and do not select the placed seek. Placed responses cannot match ordinary source-seek requests. Exact requested/actual mapping receipts are checked before display.

Source-frame selection uses the actual predecessor in source PTS. Reverse placement can map that predecessor to a project time after the request; the receipt reports it explicitly. A predecessor outside the trim is unavailable and refused. No boundary frame, physical position, source duration or observation is invented.

## Persistence and compatibility boundary

Schema 3 records definitions and active/revision/history membership. Existing schema-2 data migrate transactionally. Schema-3 missing placement tables require explicit recovery. Retain the previously validated schema-2 executable before building the new subject and test actual refusal plus a successful new-reader reopen without lost membership. Source/package/client compatibility are separate subjects.

Portable format 3 now carries this closure with fresh clone IDs and explicit origin maps, as defined in [ADR B4-004](ADR-B4-004-portable-placement-closure.md). Formats 1 and 2 omit placement metadata; the pinned prior reader refuses the new version. Placed generation is defined in [ADR B4-005](ADR-B4-005-placed-generation-sampling.md); existing-motion retiming and broader format/platform qualification remain open.

## Verification criteria

- Exact forward/reverse trim/anchor/rate round trips, signed project offsets, nonzero rate/denominator, half-open trim bounds, checked overflow and explicit sub-nanosecond quantization.
- Native projected RGB fixtures use rates +/-2/13 at a one-nanosecond project offset, preserving a denominator beyond u32 and selecting the expected source frame/eye.
- Shared desktop API and compiled CLI save and preview a 0.5-source-second / 2-project-second placement, replace it with a reverse placement, refuse the old active key, and restore membership through undo/redo/restart.
- Motion descriptor bytes and protected authored action times remain unchanged. Request replay creates no extra definition; reused IDs, stale revisions and missing Edit authority refuse. Placement-referenced source eviction refuses.
- Client matching rejects changed definitions and corrupt exact/quantized project timestamps.
- A pinned prior schema-2 engine refuses schema 3; current reopening preserves placement membership and motion. Corrupt schema-3 table loss does not silently create an empty replacement layout.

## Remaining requirements

Complete explicit existing-motion retiming and source-time Stash export for placed descendants with full lineage, cancellation and authority. Complete broader seek/encoding/platform, real-media/model and physical/audio timing gates. Synthetic local persistence and decoder checks establish their tested cases only.

The first wide-denominator fixture used +/-2/7; normalization reduced its actual denominator to 3.5e9, so it failed to exercise the intended beyond-u32 condition. The final fixture uses +/-2/13, whose normalized denominator is 6.5e9. Source-frame expectations remain unchanged. This is fixture arithmetic correction, not a media-quality threshold adjustment.
