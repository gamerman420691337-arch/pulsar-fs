# ADR B3-155: explicit six-axis audio synthesis

Date: 2026-10-12. Status: selected software contract; qualification open.

Expose audio bundles through an additive checked Protocol input binding exactly six distinct neutral-axis recipes to one immutable source version. Every recipe explicitly declares mapping, mode and spectral analysis. Reuse the existing per-axis Core mapping and receipt implementation, decode once, retain the original admitted source clock, and combine tracks only after each checked mapping completes within the original attempt deadline. Synthesized evidence, uncertainty, source/tool lineage and unqualified status remain explicit; no physical coordinate inference is added.

CLI requires --bundle-recipe JSON and refuses conflicting single-axis controls. Use the same Engine candidate/commit and exact selected-axis export contracts. Reserve one new output directory; never replace an existing directory or script. Names are axis identifiers inside this explicit bundle directory, not an accepted external companion naming standard. Six individual exports are recoverable, not an aggregate atomic transaction; an incomplete publication reports the retained committed project/revision and completed files without a success summary. Aggregate publication/recovery and desktop bundle controls remain separate unfinished work.

See [consumer scope](../../docs/PULSAR_AUDIO_BUNDLE.md) and its checkpoint. This adds usable software without qualifying audio creative quality, six-axis physical meaning, device playback, platforms, editions or human acceptance. Full roadmap remains PARTIAL.
