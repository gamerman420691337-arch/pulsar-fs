# ADR B4-007: independently captured audio with explicit unit-rate clocks

Status: locally implemented; real-media/alignment and broader qualification remain open.

## Decision

Represent a secondary audio input as an imported project source version and producer-declared corresponding original video/audio clock anchors. The engine resolves its captured artifact, binds it into immutable worker dependencies and exposes only that captured file under existing confinement. Require a distinct same-project media source. Verify bytes before and after execution and retain active dependencies against eviction.

Use checked rational `audio_anchor + (video_time - video_anchor)` under an explicit equal-rate assumption. Compare with the original decoded audio timeline before selecting the containing RMS window. Preserve video action times, visual faithful precedence and explicit creative synthesis. Missing audio support stays absent. Neither source is independently zero-aligned. No measured alignment or clock-rate repair is inferred.

Expose this declaration through the existing engine API, CLI source/alignment options and desktop imported-source selection/anchor controls. Receipts retain source identity, declaration, original/aligned clocks and contribution. Portable export/import checks the secondary source catalog/payload binding and retains original manifests as inert evidence; archived paths never execute.

## Evidence and boundaries

See [the contract](../../docs/PULSAR_INDEPENDENT_AUDIO_ALIGNMENT.md) and [local checkpoint](../../docs/evidence/roadmap-2026-10-01/independent-audio/validation.json). Authored native generation checks faithful preservation, creative exact action times, support gaps, refusal, captured-byte preservation after supplier removal, two portable clones and restart. Unit checks cover signed exact arithmetic/overflow, captured dependency integrity, active eviction holds, catalog mismatch and desktop selection. Desktop rendered usability, real-media alignment/quality, clock drift, broader fusion, human rubric and full device/platform/release gates remain open. No model training or sterile evaluation occurred.
