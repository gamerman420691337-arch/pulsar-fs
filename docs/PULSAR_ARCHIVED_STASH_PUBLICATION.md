# Stash publication from a retained generation origin

Status: implemented for a current committed master derived from one verified original imported generation anchor. Model, media, platform, installed-Stash, physical-device and human/release qualification remain open.

Subsequent [portable rebase ancestry](PULSAR_PORTABLE_REBASE_ANCESTRY.md) retains explicit unchanged-motion parents in format 9, verifies equal-clock rebases after another clone, and adds the schema-11 reader boundary. The schema-10/format-8 evidence below identifies this publication increment's original checkpoint.

Inspect the original imported candidate with `project archived-generation-status PROJECT ANCHOR --master-revision CURRENT_REVISION`. Save the selected `origins[].origin_manifest` object as bounded JSON. Then freshly bind the current scene/file to the retained source using `stash capture-source` with `--expected-source-version` and the current revision. Imported source associations are historical facts; they do not authorize this binding.

```sh
pulsar stash publish-script PROJECT SCENE FILE --url STASH_URL \
  --source-version SOURCE --candidate ORIGINAL_IMPORTED_ANCHOR \
  --job HISTORICAL_ORIGINAL_JOB --revision CURRENT_REVISION \
  --archived-origin origin-manifest.json --source-time \
  --request-id UNIQUE_PUBLICATION_ID
```

`--source-time` is required for a placed origin. An unplaced origin must omit it and uses the original elapsed-source convention. Its proof explicitly reports no verified sampled clocks and contains no invented clock mappings. The candidate argument selects the immutable original imported anchor; it is not an arbitrary edited candidate alias. Publication consumes the current committed master after checking its ancestry.

The engine repeats origin, current master, retiming and source identity checks inside the existing authorization transaction. The selected manifest reference and historical job must exactly match the retained namespace. Placed publication reconstructs the original verified source-clock plan from retained request, receipt and raw program objects, then composes the ordered verified native and archived retiming steps. Old source/model/worker paths are never opened. Historical actors and jobs stay inert; no generation job is created. Missing ancestry, altered evidence, different generation clocks and mixed live/archive correspondence refuse.

Existing local-owner capability, current grants, revision checks, fresh exact scene/file observation, opened source descriptor identity, private proof retention and atomic no-clobber sibling publication still apply. Exact request replay returns the stored outcome without a Stash query. Query-only publication observation checks the stored receipt, proof metadata and retained proof context together. Pending or unknown dispatch never authorizes a resend or becomes complete merely because files can be observed.

Private proof schema 3 records explicit archival origin and current master derivation. The export receipt's `job_id` is the original historical identity when `timing_proof.archived_context` is present; that context explicitly sets `archival=true`, `active_job=false`, and `release_qualified=false`. Native proof schemas 1 and 2 retain their existing representation. The first archival publication outbox transaction advances the database reader boundary to schema 10 before dispatch. Fresh and native-only databases retain schema 9; reopening schema 10 preserves it. Portable project format remains 8. The pinned preceding schema-9 reader refuses these new stored proof subjects before writable database initialization.

The desktop provides an explicit archived-generation toggle and verified namespace selection. It requires a master report matching the visible project, candidate, revision and motion identity. Controls compile; rendered usability remains unverified.

The [local validation receipt](evidence/roadmap-2026-10-01/archived-stash-publication/validation.json) covers authored forward/reverse generation, retained retiming, manual edits across two clones, unavailable original worker directories, missing fresh binding, wrong namespace, damaged retained receipt, missing source-time selection, existing destination refusal, no imported jobs, restart/replay, older-reader refusal, and unplaced proof semantics. These are synthetic native fixtures against an owned query-only mock Stash server. They do not establish real-media quality, independent original execution, actual Stash attachment or physical Handy operation.

Richer mixed-clock correspondence, legacy absent rebase facts, GUI qualification and the remaining full roadmap acceptance packages remain required work.
