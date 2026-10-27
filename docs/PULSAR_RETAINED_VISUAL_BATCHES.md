# Retained visual/model library batches

The shared local batch ledger accepts a checked visual recipe through `library-batch-prepare FOLDER --visual-recipe /absolute/recipe.json`. The desktop library panel accepts the same recipe through its JSON-file intake. A visual recipe contains `kind: "visual"`, `preset`, explicit `settings` and `model_path` (an absolute path or null). Settings follow the shared GenerationSettings contract; supplied models require an explicit tensor contract, and optional declared profiles retain their measured model/runtime identities. Audio recipe JSON and the audio CLI option retain their original representation.

Intake preserves existing standalone companion scripts and existing visual bundle directories. New visual destinations use the original media filename followed by `.pulsar-visual-bundle`. Each visual item records only Stroke for publication. This does not reconstruct secondary physical axes or qualify motion quality.

The Engine snapshots every selected source and the optional model into its own content-addressed storage before publishing the retained batch. Generation uses those captured bytes, original settings, Worker/tool identity and ordinary generation request identities. Advancing, waiting for a completed candidate, committing and publication remain separate retained phases. Restart and exact request recovery must reuse those phases rather than create replacement work.

Visual records use report schema 3 and storage reader floor 30. Their model membership is separate from source-item membership. The reader barrier survives ordinary inner-storage updates and cancellation; metadata release removes the owned batch memberships rather than deleting media, model, projects or published outputs. Older Engines refuse this reader floor. Audio batches retain their existing schema 1/2 semantics even in a database whose newer reader floor is armed.

Publication uses the selected-axis publisher's original private stage/request IDs, exact master verification, strict script comparison and no-replace directory rename. A workflow result and each exported receipt remain `qualified: false`. Missing models, unsupported settings, changed captured bytes, changed tool identity and unresolved effects must not be represented as a successful batch.

Verification is recorded in the [checkpoint](evidence/roadmap-2026-10-12/retained-visual-batches/checkpoint.json). This implementation does not itself establish annotated-media accuracy, qualified model defaults, native rendered GUI or Handy/human/platform/release acceptance.
