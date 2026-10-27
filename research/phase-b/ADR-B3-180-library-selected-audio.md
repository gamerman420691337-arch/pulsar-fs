# ADR B3-180: retain audio output-axis selection independently of synthesis

Status: accepted software design; partial-bundle filling and qualification open.

TEST-15 identifies existing stroke plus missing secondary channels. Audio intake
previously preserved the adjacent stroke file but published a second synthesized
stroke in its new neutral bundle. Whole-entry presence and generation-family
inference cannot accurately describe a partial output choice.

Capture an optional canonical output-axis subset per input, separate from the
six-channel synthesis recipe. For an observed adjacent regular stroke, shared
retained intake selects the five secondary axes. Validate the six-axis generation
master at commit and the selected subset at publication. Preserve missing-field
legacy bytes; refuse noncanonical/empty/duplicate/full subsets and visual misuse.

Use explicit selected-audio report/journal schema 5 and outer reader floor 33.
Cancellation and migration retain the output selection; immutable journal and
desktop acknowledgement checks reject changed axes. Avoid inferring visual
generation from a schema number or from the presence of selected axes.

Use the existing private no-replace neutral-bundle publisher for a new destination.
This directly preserves adjacent stroke and supplies missing secondary files in
the recognized bundle location. It does not implement merging into an already
partial bundle, infer script validity, or classify an internal generated axis as
a published user output. Existing-bundle filling remains in the original scope.

See [contract](../../docs/PULSAR_LIBRARY_SELECTED_AUDIO.md) and
[evidence](../../docs/evidence/roadmap-2026-10-12/library-selected-audio/checkpoint.json).
