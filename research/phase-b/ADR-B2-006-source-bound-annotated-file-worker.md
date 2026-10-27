# B2-006: Source-bound annotated file worker

Status: Implemented at worker boundary; Engine publication remains disabled
Date: 2026-10-08

## Evidence and decision

The annotated export names its input `target_frame_pair` and declares a selected native eye/projection plus static integer region before nearest 64×64 scaling. The existing temporal worker instead resizes the entire packed source and expects `frame_pair`. Selecting a model by shape or silently reinterpreting the old preprocessing enum would execute a different recipe.

Add explicit `annotated_temporal_view` preprocessing and optional `GenerationSettings.annotated_view`, containing a source SHA-256 and the shared annotated declaration. Both are required together. Source content, encoded dimensions, geometry and intermediate buffers are admitted before decoding. The worker calls the same numeric planner used by training, and a transform digest binds the source, declaration and complete plan. The model adapter validates the exact exported metadata and uses the existing request-owned CPU session/deadline instead of constructing an unowned production session.

The worker emits schema-3 lineage with the exact plan, transform, settings, model recipe and training-intake digest. Original source clocks, sampling lattice and existing gap behavior remain intact. A static region is not target tracking or anatomical ground truth. CUDA and raw/live frames are refused for this recipe until their own adapters are implemented and verified.

## Compatibility and remaining integration

Absent optional settings serialize exactly as before; existing preprocessing variants and whole-frame recipes keep their meanings. Older typed readers refuse the new enum/field. Current portable receipt validation also refuses schema 3. This is insufficient to establish safe database startup/recovery or portable closure, so Engine submission and internal attempt preparation explicitly refuse annotated generation. The public refusal occurs before snapshot preparation. Generic profile admission (including Stash batches), model-pack validation and selected-pack execution also refuse it. Only source-bound file-worker admission executes the recipe.

Before removing those guards, implement conditional durable reader barriers, bounded read-only startup validation, retry/origin/receipt correspondence and portable import/export verification. Then expose explicit first-party CLI and desktop controls and run actual public Engine generation/restart/portable campaigns. Do not auto-activate or infer an annotated profile from model metadata. This staged decision does not close those roadmap requirements.

## Verification and rollback

Authored stereo file verification checks every selected output pixel and original sample order, compares 14 inferred positions with retained Python results and checks schema-3 plan/model lineage. Invalid source dimensions refuse before artifact/session creation. Protocol tests cover absent/conflicting declarations and exact normalization/provider constraints. Public Engine refusal checks unchanged snapshots, job/attempt counts and database version. Broad regression gates and exact source identities are recorded in the [checkpoint](../../docs/evidence/roadmap-2026-10-08/annotated-file-worker/checkpoint.json).

Rollback restores the preceding source cohort. No database migration, model activation, historical input mutation or physical actuation is performed by this checkpoint. Real target annotations/alignment, quality, platform, Handy, human and release qualification remain open.
