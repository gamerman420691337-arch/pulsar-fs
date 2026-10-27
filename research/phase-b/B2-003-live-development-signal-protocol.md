# B2-003: Live-development position signal baseline

Status: Frozen before measurement; diagnostic only.
Date: 2026-10-08

## Question and subjects

Does the existing permitted live-reference development dataset provide cross-group position signal for a fixed image retrieval baseline, and are there exact duplicate temporal inputs with contradictory supplied labels? Use only the hash-bound dataset, intake, split and frozen model in `.pulsar-reference-evidence/live-position-holdout-2026-10-04-retry/`. These groups are already consumed development data, not an independent holdout. Do not read sterile files, refit a model, choose a checkpoint or change labels/alignment.

## Fixed measurements

1. Verify dataset, model and split/receipt identities. Preserve the original six optimizer groups (2,280 samples) and two development groups (580 samples). Verify source-byte separation and report exact temporal-input duplicates across the split; distinct bytes do not prove source-family independence.
2. Hash each original uint8 six-channel 64×64 frame pair. Report duplicate groups, cross-split duplicate inputs, and the maximum supplied-label spread within identical inputs. No perceptual duplicate threshold or rejection policy is introduced.
3. Construct one fixed descriptor: mean of each nonoverlapping 8×8 block in each channel, retaining the 8×8 spatial grid and previous-RGB/current-RGB channel order, divided by 255. Use Euclidean nearest neighbor with stable original training-row tie order. No fitted normalization, feature search, k search or label-dependent neighbor selection.
4. Predict all 580 development positions from the 2,280 optimizer examples. Compare aggregate/per-group MAE with the constant optimizer-label mean/median and the existing frozen ONNX candidate. The baseline does not fit or export a model.
5. As descriptive controls, predict optimizer rows from other optimizer groups, and from their own declared source group while excluding all candidates less than one second away in exact source PTS. These controls are not independent validation or model selection.
6. Bound CPU threads to four, keep raw per-sample details in a new private directory, and retain source/runtime/protocol identities and aggregate diagnostics. Repeat the fixed run once to check metric reproducibility. Test nearest-neighbor arithmetic and mask/tie behavior on authored toy arrays before using the dataset.

## Interpretation and stop rules

Any useful baseline result establishes only supplied-position correlation under this fixed development comparison. It does not establish target identity, verified label alignment, stroke matching, model generalization, physical motion or release readiness. Poor retrieval does not prove the task intrinsically impossible. Model and labels remain unqualified regardless of this outcome. Reject missing/changed identities, bad shapes/nonfinite labels, incomplete split membership or an empty admitted neighbor set; do not repair the data. Do not tune or repeat alternative settings in response to these metrics. Next research must retain this dataset's consumed status and use training-only inputs; any acceptance evaluation requires separately justified evidence.
