# Source-bound placement and interval batch contract

Status: implemented Engine/CLI workflow; qualification remains evidence-bound.

`StashBatchSourcePlan` describes one exact Stash scene/file, source SHA-256,
optional immutable placement specification, optional half-open project interval
and explicit source-time publication intent. It carries no placement ID or
revision. Those must come from an acknowledged Engine placement transaction.

The shared validator admits at most 100 unique bounded keys, refuses shared
placement/interval settings and raw frame-pair interpretation, and validates
each ordinary generation setting before source capture. A nonidentity placement
requires explicit source-time conversion. Unplaced source-time conversion
refuses. Identity-clock placements may retain ordinary publication intent.
Annotated view catalogs and source plans must cover identical scene/file keys
and source hashes; ordinary whole-frame plans need no annotated catalog.

The resolver binds the selected plan to the captured source hash/version and
requires the exact acknowledged placement specification, preserving its actual
ID and creation revision. It composes the interval, placement and optional
annotated view into an ordinary validated `GenerationSettings`, without changing
the template. The Engine remains responsible for active project membership,
revision/grants, immutable media custody, interval/sample support and verified
source-clock receipts. This protocol helper cannot authorize those facts.

## Engine workflow and first-party controls

Batch options and full replacement migration intent accept an optional
`source_plans` array. An empty catalog omits the new wire field and retains the
preceding behavior. Nonempty catalogs must cover every eligible selected file,
with no extra ineligible/unselected entries, before model snapshotting. A
capture must match the requested source content and placement source version
before any placement or generation effect can be recorded.

The scheduler journals `SetSourcePlacement` before dispatch. Its acknowledged
project snapshot supplies the actual active placement and next revision. A
second journaled effect generates using those exact settings. The accepted job
retains its source plan and publication clock independently of later options.
Source-time publication uses the existing verified original-sample clock path;
no project-time script is silently written beside a differently timed video.

The CLI accepts `--source-plans PLANS.json` on `stash generate-missing`; desktop
offers an optional source-plan catalog path beside the annotated-view path.
Both use the existing no-follow, regular-file, 64 KiB JSON boundary. File models,
currently selected model packs and optional annotated catalogs share the same
Engine workflow. Annotated CPU execution and [owner-selected annotated CUDA packs](PULSAR_ANNOTATED_CUDA_PACK_BATCHES.md) have authored native evidence. [File-model CUDA placed/interval batches](PULSAR_ANNOTATED_CUDA_STASH_BATCHES.md) now also have authored native evidence.

Each `PLANS.json` entry contains exact `scene_id`, `file_id`, `source_sha256`,
optional `placement` (`SourcePlacementSpec`), optional `project_interval`
(`start`/`end` project nanoseconds) and required boolean `source_time`. Actual
source identities and authored timing are required; no placement ID/revision
belongs in the input catalog. For example:

```sh
pulsar stash generate-missing --url http://127.0.0.1:9999/graphql \
  --mappings MAPPINGS.json --source-plans PLANS.json \
  --width 64 --height 64 --preset fast --request-id BATCH_ID --detach
```

## Recovery, migration and reader fencing

Placement effects replay their exact request identity after restart. Migration
remains owner-authorized, stopped, ledger-digest-bound and paused until explicit
resume. It reconciles authoritative placement acknowledgements before changing
future intent. An accepted old placement is explicitly replaced or removed when
future settings require it. Unaccepted placement/generation requests are
retained as superseded history with separate receipt lists. Accepted jobs keep
their immutable settings and publication clock; migration never rewinds them.
Project revisions, edit/generation/import grants and selected-pack approval
remain mandatory.

Source plans use batch ledger/report version 6 and atomically require the
existing schema-13 read-only batch validator before writable recovery. Current
and historical catalogs, placement acknowledgements and frozen generation
intent retain this requirement. Existing later storage wrappers remain intact.
Startup validates exact pending commands, project/revision/effect identities,
catalog correspondence and retained generation intent. Missing frozen intent
for a placed accepted job refuses. Historical empty catalogs remain compatible.

## Publication boundaries and evidence

An interval can contain times with no decoded support. Those boundaries remain
explicit motion gaps; publication refuses rather than extending or discarding
them. A review decision alone cannot create physical support. The retained
project/job permits explicit recovery; the batch does not invent actions or
silently replace an accepted job. Mixed-clock piecewise publication remains
outside this workflow and requires separate correspondence proof.

The protocol regression suite covers acknowledged placement and half-open
interval preservation, source/key/spec substitution, explicit clock intent,
catalog bounds/duplicates, strict wire parsing, annotated composition and
cross-source refusal, reverse target seeds, negative intervals and raw pairs.
The protocol foundation is retained under
`docs/evidence/roadmap-2026-10-08/stash-source-plan-contract/`. Integrated
results, authored native campaigns and exact sources are retained separately
under `docs/evidence/roadmap-2026-10-08/stash-source-plan-batches/`.

The integrated workspace passes 1,121 tests (63 ignored); all 23 Stash batch
tests also pass in the CUDA-feature build. The structural gate passes 288 checks
and the source guard suite passes 36. Five new Engine tests exercise durable
placement ordering, actual lost-acknowledgement replay across reopen, accepted
placement reconciliation followed by replacement/removal, unaccepted effect
supersession, accepted-job clock retention, capture/coverage refusal and reader
corruption. The first-party source-plan file loader has a separate regression.

The authored native ordinary campaign publishes five exact source-clock
actions per forward/reverse interval, verifies scan/tag once per publication,
restarts at a durable placement intent and restores both portable projects into
fresh missing-script library entries with identical scripts. Full placement
clock receipts retain eight anchors while intervals select five actions. A
wrong-hash third source requires digest-bound paused migration and explicit
resume; a fourth source retains uncovered boundary gaps and refuses publication
with no scan/tag effect. Original interactive scenes/scripts remain intact.

The separate annotated selected-pack campaign verifies two different sources,
views, placements and intervals, with 42 Python-corresponding positions across
three completed jobs. It retains three 28-action source-time proofs, restart,
exact approval dependencies, source/hash repair and owner withdrawal refusal.
Both new campaigns observe the preceding executable refuse the new ledger
without changing database/WAL bytes. Existing unplaced file/pack campaigns are
run separately; their already-supported old format is not an older-reader
negative control for this change.

No user Stash service, real media, model fitting, sterile evaluation, production
database migration or physical playback is used by these authored campaigns.
Actual desktop window interaction, installed Stash compatibility, broader
fault/retention qualification and the model/platform/Handy/human acceptance
gates remain open.
