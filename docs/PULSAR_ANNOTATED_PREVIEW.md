# Source-bound annotated CPU Preview

Status: implemented and verified against authored CPU fixtures. Roadmap PARTIAL.

The explicit `PreviewAnnotatedFrame` request previews target pixels and an original-source frame-pair estimate from either a file model/profile or the currently owner-selected annotated CPU pack. Legacy single-frame detector Preview commands retain their previous contract and continue to refuse temporal profiles.

## CLI

```sh
pulsar project preview-annotated PROJECT_ID 125000000 \
  --source-version SOURCE_VERSION --annotated-view VIEW.json \
  --model-pack MANIFEST_SHA256

pulsar project preview-annotated PROJECT_ID 125000000 \
  --source-version SOURCE_VERSION --annotated-view VIEW.json \
  --model MODEL.onnx --inference-profile PROFILE.json
```

The CLI time is elapsed source nanoseconds. With `--placement-id PLACEMENT_ID` instead of `--source-version`, it is project time resolved through the active placement. The RPC additionally supports explicit source-PTS time. `--source-sample-stride` defaults to one. Preview chooses the latest indexed source frame at or before the requested source time, then the latest original stride sample at or before that frame. The predecessor is exactly one declared stride earlier in original source order, including reverse placements; no frame pair is reversed to follow project playback.

`VIEW.json` is the same exact source-hash/encoded-dimensions/eye/projection/target-region declaration used by annotated generation. Pixels use the same native crop/resize plan and named ONNX frame-pair recipe. The returned current and predecessor target images are exact 64x64 RGB. Source PTS, source origin, elapsed time, actual frame ordinals, target transform and placed-time quantization remain explicit.

## Evidence and absence

Temporal position is separate from detector observations: annotated Preview returns an empty observation list and no detector coordinate map. The evidence carries the original view/stride, model, canonical admitted profile, runtime, ONNX recipe and training-intake identities, predecessor pixels and optional position. The canonical profile identity is distinct from a supplier profile file's byte identity.

Generation's source-clock continuity rules apply. The first sample has no previous pair and returns `no_previous_sample`. Scene cuts, nonpositive deltas and deltas greater than 500 ms return `temporal_pair_without_continuous_support`. These cases retain current pixels and an unavailable estimate; Preview does not invent a neutral position or detector box. All estimates remain unqualified.

The Engine requires Read and ImportSource grants, checks imported-media content binding before model access, and rechecks current selected-pack approval before execution and completion. Both pixel artifacts cross bounded digest-checked worker egress into readonly host attempt files. Before returning, the Engine verifies model/recipe/profile/runtime/view correspondence, source/pair clocks, pixel identities and actual photometric/continuity support. Preview creates no generation job, candidate, committed motion edit or authoritative attempts row. It does retain owned worker output files for the preview execution.

## Verification

The shipped CLI/local-IPC campaign passes all 14 independent Python positions with zero difference and all 14 current/predecessor RGB pairs byte for byte. The authored source starts at PTS 2 seconds. Explicit source-PTS requests and asymmetric forward/reverse placements resolve correctly: at project 875 ms, forward selects source frame 7 with predecessor 6, while reverse selects frame 5 with predecessor 4. First-frame, scene-cut and one-second-gap estimates remain absent. Restart, wrong-source refusal before model access, wrong-context refusal, installed-model drift and withdrawn owner approval pass. Preview creates no generation jobs or authoritative attempts and leaves the master motion program unchanged.

The final workspace gate passes 1,104 tests with 62 ignored. CUDA-feature annotated contract checks pass 27 tests with five ignored; these do not execute an annotated CUDA adapter. The native Preview campaign and four native pack/Stash regressions pass. The default build, 286 architecture checks and 36 source guards pass. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-preview/checkpoint.json) and [ADR B3-079](../research/phase-b/ADR-B3-079-annotated-preview.md).

## Limits

This is a CPU software capability. Annotated CUDA, actual desktop window rendering and human usability, per-source placed/interval batches, mixed-clock publication without piecewise proof, broader fault/retention qualification, real annotation/alignment and useful held-out model quality, installed Stash compatibility, platform/offline distribution, physical Handy playback and human/release acceptance remain open. No live or sterile training input, fitting, external Stash service or physical playback is used by this campaign.

[Desktop annotated Preview wiring](PULSAR_ANNOTATED_DESKTOP_PREVIEW.md) now presents explicit target pairs and estimates with separately verified presentation and native controller checks. Native window rendering and human usability remain open.

The subsequent [annotated CUDA preview checkpoint](PULSAR_ANNOTATED_CUDA_PREVIEW.md) closes authored native CPU/CUDA preview parity, including explicit initialization-only evidence for unsupported pairs. The earlier CPU checkpoint and its recorded limits remain historical evidence.
