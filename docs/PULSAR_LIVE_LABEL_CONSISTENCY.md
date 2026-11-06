# Live-reference label consistency and sampled motion

An independent audit rebuilt every accepted label from the saved source PTS, declared origin/offset and original supplied script. **All 2,860 stored float32 labels match exactly**, including all 2,280 optimizer and 580 consumed development examples. Ordered entry IDs, PTS pairs, clip omission counts and hash-bound references also agree. This rules out reconstruction discrepancies in these stored rows; it does not verify source pixel/PTS correspondence, human script alignment or physical target identity.

The same fixed audit measures supplied-script detail between each pair's endpoint times:

| Measurement | Result |
| --- | ---: |
| Pairs with a strict interior script turning knot | 1,293 |
| Strict interior turning knots | 1,572 |
| Pairs with script excursion outside their endpoint positions | 1,287 / 2,860 (45%) |
| Largest normalized hidden excursion | 0.820482 |
| Mean full script polyline variation per pair | 0.376645 |
| Mean endpoint displacement per pair | 0.192298 |
| Pair duration range | 66.733–166.833 ms |

The largest excursion is expressed in normalized supplied script position, not measured physical travel. Strict turns exclude plateau extrema and interval boundaries. Hidden excursions can therefore occur without a counted strict turn. These measurements expose detail absent from endpoint positions at the existing sampling cadence; they do not prove why the learned candidate underfits, establish anatomical observability or imply every temporal method must fail.

The bound training manifest has no declared target-box or projection annotation fields. Its preprocessing is nearest resizing of the entire packed source to 64×64, with previous/current RGB concatenation. That declaration does not establish useful target identity across differently shaped sources. No visual content was inspected or decoded for this audit. The absence of annotation declarations is not a search for unrecorded annotations elsewhere.

The [predeclared protocol](../research/phase-b/B2-004-live-label-consistency-protocol.md) fixes inputs, arithmetic, scope and stop rules. Two runs produce identical results. Four authored tests cover exact interpolation, nonzero source origin and offset, strict interior and plateau semantics, deliberate label corruption, changed rows and changed reference identity. A separate linear-segment implementation checks a predetermined row from each entry; aggregate and split count readback agrees. The new [audit utility](../scripts/audit-reference-labels.py) reports mismatches without repairing the dataset and never loads image tensors, fits a model or reads sterile inputs.

[The source-bound packet](evidence/roadmap-2026-10-08/live-label-consistency/checkpoint.json) retains input/runtime/protocol identities, aggregate results and independent checks. All 369 preceding source records are unchanged; the two new verification scripts bring the snapshot to 371 records. Rust/CUDA gates were not rerun because their sources and runtime behavior are unchanged. No model, label, offset, activation or physical dispatch changed.

Next work needs explicit target/projection-aware training inputs, pixel/clock correspondence and independently justified visual/script alignment. The eight groups remain consumed training/development data. Useful learned models, independent sterile/real-media acceptance, broader product and Stash closure, platform, Handy, human and Preview/Stable release gates remain open.
