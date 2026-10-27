# Reference-model capacity and fixed longer-training experiment

Status: software checks pass; useful model quality still fails. Roadmap PARTIAL.

The private whole-frame candidate's negative development result is not repaired by fitting longer. A fixed optimizer-only capacity diagnostic and a separate fixed 100-epoch run now distinguish memorization from the existing development and native stroke measurements. Live references are training/development inputs. Sterile references remain untouched; no model is activated or distributed.

## Capacity controls

`scripts/diagnose-reference-capacity.py` admits the existing hash-bound training manifest, intake, dataset and declared development split. It snapshots bounded regular files into a new private directory and records selection, controls, source identities and stop rules before fitting. Row choice is seeded, balanced across optimizer entries and independent of label values. Development rows cannot enter either optimizer control. The shared 49,201-parameter CPU network, Adam settings and normalization are unchanged from the existing trainer. A constructor readback verifies all ten initialized parameter tensors and an authored output exactly against the preceding implementation.

The fixed run selected 128 rows from six optimizer entries, excluding all 580 development samples. Original-label and seeded permuted-label controls each ran 100 epochs. No checkpoint was selected, saved or exported by this diagnostic.

| Control | Subset MAE at epoch 10 | Subset MAE at epoch 100 | Constant-mean subset baseline |
| --- | ---: | ---: | ---: |
| Original labels | 0.175972 | 0.059496 | 0.197426 |
| Permuted labels | 0.191622 | 0.046042 | 0.197426 |

The network optimizes and can memorize even shuffled labels. These are resubstitution statistics, not evidence of meaningful visual alignment or generalization. An exact-input audit finds no duplicate pair or current-frame tensors among all 2,860 already consumed rows; that does not rule out near-duplicate ambiguity or information lost by sampling/whole-frame resizing.

## Fixed longer candidate and controlled comparison

A separate protocol fixed 100 epochs and the original seed 20261004 before fitting, retained the original six optimizer/two development groups, and measured only the final checkpoint. The development groups were already consumed data. This is not a fresh independent test or a release evaluation. The new private model is `9a64cc4b5643c7c64d5d6739485c13418fa50e433ea49ee439ed4481f2dfc251`; the original model and weights remain unchanged.

| Measurement | Original 10 epochs | Fixed 100 epochs |
| --- | ---: | ---: |
| Optimizer MAE | 0.165076 | 0.032632 |
| Development MAE, same current ONNX Runtime | 0.285795 | 0.295373 |
| Constant training-mean development baseline | 0.257179 | 0.257179 |
| Native supplied reference edges across eight clips | 242 | 242 |
| Native predicted monotone strokes | 404 | 331 |
| Matching strokes under unchanged scorer | 0 | 0 |

Both frozen models were measured with the same current CPU runtime. Each then executed all 580 inference runs through the same current confined production worker and benchmark runner. The eight paired inputs have identical original RGB hashes, rational PTS and reference documents. Existing recorded-actions windows and scorer criteria were unchanged. Every case completed and scored. Counts describe supplied-script comparisons, not independently measured physical motion. The new export's maximum PyTorch/ONNX discrepancy is 3.2783e-7 on optimizer parity samples and 5.6625e-7 across all development samples.

The longer model fits optimizer labels much better but performs worse on both development groups and still produces no matching strokes. This result does not identify a single cause. Explicit target/projection inputs, justified script alignment, sampling adequacy and model design remain unresolved; extra fitting alone has not solved the observed gap. No result here authorizes promotion.

## Verification and retained limits

Five new authored regressions cover balanced selection without development leakage, duplicate-input conflict bounds, deterministic fitting of an authored signal, actual two-control execution with output preservation, and changed dataset/split refusal before fitting. The complete reference Python suite passes 36 tests, including actual legacy and annotated ONNX export/native-planner checks. Both JavaScript worker-campaign tests pass with the authored native case explicitly enabled. The benchmark runner builds, 286 architecture checks and 36 source guards pass. Rust sources and the production worker binary are byte-identical to the preceding desktop checkpoint; workspace/CUDA gates were not rerun for this Python experiment.

The [source-bound checkpoint](evidence/roadmap-2026-10-08/reference-capacity/checkpoint.json) retains protocols, aggregate measurements, exact source/runtime/input identities and terminal logs. Private tensors, detailed intake, original source names, derived media, models and weights remain in the ignored reference-evidence directory. Existing runs are preserved. An initial regression invocation used stale cache paths and could not import ONNX packages; the corrected invocation uses existing installed cache entries without downloads. An initial example-build invocation omitted the package selector; the corrected package-scoped build passes.

Useful held-out models, independently justified natural-media annotations/alignment, calibrated tracking/fallbacks, annotated CUDA, remaining product/batch work, actual desktop/Stash/platform/offline qualification, physical Handy and human/release acceptance remain open.
