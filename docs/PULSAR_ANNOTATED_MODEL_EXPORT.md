# Separately bound annotated temporal model export

Annotated training-view datasets now have their own training/export entry point. It requires schema-2 intake, exact manifest/annotation/dataset identities, the original native planner identity and an explicit development-group split. Before fitting, it replays numeric native planning and rejects any saved-plan discrepancy. Saved filter strings are never executed by this driver. Source-group/byte separation, sample counts, array types and source-pair gaps are checked; family identity and visual/script alignment remain unverified.

```sh
python3 scripts/train-reference-views.py \
  --staged-run PRIVATE_ANNOTATED_RUN \
  --planner /absolute/path/to/original/pulsar \
  --output NEW_PRIVATE_MODEL_RUN \
  --heldout-group DECLARED_DEVELOPMENT_GROUP
```

The output directory must be new. The driver snapshots intake, manifest, annotation and dataset bytes, records the fixed split and invocation, and writes the model contract. It fits only optimizer groups and measures the declared development groups without development-based checkpoint selection. This is a developer training workflow, not a portable product-runtime dependency claim.

The new ABI is named float32 `target_frame_pair`, fixed `[1,6,64,64]`, previous RGB then current RGB divided by 255, producing float32 `script_position [1,1]` in `[0,1]`. ONNX metadata `pulsar.temporal_recipe` carries the versioned `annotated-temporal-view-v1` contract; `pulsar.training_intake_sha256` binds its intake. The model contract is source-independent: individual source/view/region declarations remain bound in intake and must be explicitly supplied to a future generation attempt. Shape/name/type, metadata, model digest and PyTorch/ONNX parity are checked after export. The network architecture is unchanged from the small existing candidate; no quality improvement is inferred.

Whole-frame export still uses `frame_pair` and schema-1 receipts. It refuses annotated intake, including attempted schema-only downgrades that retain annotation markers. The existing production temporal adapter still requires `frame_pair`; annotated production profile/runtime/generation binding is not implemented by this checkpoint. No activation or implicit model substitution occurs.

Verification uses authored media only: two declared groups produce seven optimizer and seven development pairs. One fixed epoch exports the separately named model. Optimizer ONNX parity is 5.960464477539063e-8 and development parity is 2.9802322387695312e-8, both below 1e-5. Direct old-input-name execution refuses; changed saved planning refuses before fitting; retry preserves an existing output. Independent readback verifies recipe/intake/model/source identities and executes all 14 authored positions. The complete reference Python suite passes 31 tests, including legacy training/export, staging, admission and label/pixel audits. All 275 architecture checks and 36 source guards pass. Rust/CUDA/workspace gates were not rerun because their sources are unchanged.

[The evidence checkpoint](evidence/roadmap-2026-10-08/annotated-model-export/checkpoint.json) retains source/archive readback, exact test results, runtime identities and hashes for the private authored fixture. The archive has 378 source records: 375 preceding paths unchanged, one existing trainer changed and two new verification/driver paths. An initial missing cached `ml_dtypes` dependency failure is retained; only terminal successful gates count. No package download, live/sterile fitting or existing dataset/model/reference change occurred.

These fixture measurements establish exporter behavior, not useful learned quality or independent acceptance. The next software requirement is explicit annotated model/profile/runtime/generation binding through the same native view planner. Permitted real annotations, independent visual/script alignment, dynamic target behavior, useful model quality, broader Stash/product closure, platform, Handy, human and release qualification remain open. The roadmap remains PARTIAL.
