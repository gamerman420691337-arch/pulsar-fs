# Stash integration in the current engine

Pulsar clients send typed requests to the authenticated engine. The engine owns fixed GraphQL operations, network deadlines, response/pagination limits and credential resolution. Stash discovery selects scenes with `interactive: false`; this flag can be stale and does not validate a script. Every request also requires an independent host-local Stash owner capability. The CLI and desktop read that capability from the private engine directory.

## Discovery

```sh
pulsar stash list-missing --url http://localhost:9999/graphql --limit 50
```

`stash query` is a compatibility alias. For authenticated Stash, provide the environment variable name, for example `--api-key-env STASH_API_KEY`. Its value must already be available to the engine: start the engine with `secrets run -- pulsar engine`. An already-running engine retains its own environment; changing the client's environment does not add credentials to it. Never pass a plaintext key on the command line. Pulsar rejects URL credentials, query parameters, redirects, environment proxies and cleartext remote endpoints; remote servers require HTTPS.

Discovery returns at most 100 scenes over four bounded pages and reports whether pagination completed. Counts are advisory because Stash does not supply a transaction snapshot here. Multifile scenes are displayed but cannot be automatically selected by list order.

## Local-file planning

Stash paths belong to the server's namespace. Supply a JSON array mapping server POSIX roots to host folders:

```json
[
  {"server_root":"/stash/media","local_root":"/home/user/media"}
]
```

```sh
pulsar stash plan-missing --mappings mappings.json --limit 50
```

The desktop provides a mappings-file field and the same planning operation. Each scene reports an explicit status. `ready_for_snapshot` means one unambiguous mapped regular source currently exists and no sibling `.funscript` entry was observed. This is a mutable observation, not captured input identity or permission to overwrite. The Unix planner opens every directory/source component without following symlinks. Any existing script entry, including a dangling symlink, blocks readiness. Prefix collisions, traversal, overlapping mappings and multifile scenes are rejected. Non-Unix file inspection currently reports `unsupported_platform`.

Planning performs discovery and local-file observation only. It does not generate scripts, import media, upload/download files, scan Stash, create tags or replace an existing script. A generation pipeline must independently snapshot the chosen source and publish atomically without clobbering an existing destination.

## Source capture

```sh
pulsar stash capture-source PROJECT_ID SCENE_ID FILE_ID --mappings mappings.json
```

The desktop can capture a ready source into the current project. The engine independently re-queries the scene, verifies the selected file ID, rechecks the mapped namespace and script absence, and captures the opened regular-file descriptor into its private content-addressed store. A changed path cannot redirect an already-open descriptor. Capture requires the local owner capability, the project's ImportSource grant and an expected revision. The CLI obtains the revision from the project, or accepts explicit `--revision` for callers without Read permission.

The returned `stash_source_captured` receipt retains source hash/byte length, scene/file IDs and distinct server/host/script paths. Source bytes and the association receipt are durable; successful request replay returns the recorded outcome without re-querying the server and rechecks current authority. Later edits to the original source cannot alter the snapshot. No script is published and no scan or tag mutation occurs during capture.

## Generation and publication

The [live generation clock guard](PULSAR_STASH_GENERATION_CLOCK_GUARD.md) verifies each contributing job against the selected source and exact declared clock. Equal-clock independent generations remain publishable; differing placements require explicit piecewise correspondence and refuse. Matching source hashes alone do not make generation clocks interchangeable. [Native origin verification](PULSAR_NATIVE_CLOCK_ORIGINS.md) now centralizes bounded source/job/candidate/receipt and stored-motion checks for the existing native path. [Explicit per-action clock arithmetic](PULSAR_SELECTED_SOURCE_CLOCKS.md) is implemented in core; Engine ancestry verification and durable mixed-clock publication proofs remain pending, so differing-clock publication still refuses.

```sh
pulsar stash generate-missing --mappings mappings.json --limit 50
pulsar stash publish-script PROJECT_ID SCENE_ID FILE_ID --source-version SOURCE_ID --candidate CANDIDATE_ID --job JOB_ID --revision REVISION
```

The desktop provides the same batch flow using Generator model/preset settings and a current-project publication button. Batch generation materializes the selection before writing scripts, then creates separate projects, captures immutable sources, runs existing engine-owned generation jobs, commits candidates and requests publication. `--video-view FILE.json` declares a shared stereo/spherical analysis view for CLI batches; desktop batches use the Generator view controls. See [declared video views](PULSAR_VIDEO_VIEWS.md). Explicit model/profile arguments remain required for trained inference; no model is downloaded automatically. Without a model, global motion inference remains unqualified.

Publication independently verifies the Stash endpoint, scene/file identity, captured source bytes and source/job/candidate correspondence at the committed revision. It requires Export authority and expected revision. Unix descriptor-relative publication rejects symlink paths and source/directory replacement, stages and synchronizes bytes, and atomically creates the sibling script without replacing an existing entry. It verifies published bytes and durably records the outcome. The receipt reports review uncertainty and always declares `release_qualified: false`. Unresolved selected-axis motion gaps block publication.

The batch reports per-item project/source/job/candidate/request IDs and partial or unknown failures. Exact successful publication replay rechecks authority and returns its durable receipt without another server query. Unknown dispatch outcomes require reconciliation before another effect; no automatic retry is performed. Batch scheduling and its phase ledger now belong to the engine; client waiting is observation only. Publication now accepts recorded local descendants of the selected captured-source generation candidate. The engine follows revision links for edits/protection, the restored revision for undo/redo, and recorded stroke contributions for merges. A replacement commit follows the replacement candidate rather than its historical parent. Immutable edit candidates follow their original upload receipt's base revision, including rebased edit proposals; newly rebased generation candidates retain an explicit parent-candidate link. Foreign-source generation, unrelated imported scripts, unknown legacy lineage, ambiguous receipts and cycles are refused. Equal program bytes never establish origin. Traversal is bounded to 128 levels and 2,048 nodes; exceeding a bound reports failure, not successful provenance. This is source association, not proof of motion accuracy, human acceptance or exact per-sample origin. Historical rebases without recorded parent links and portable Stash association migration remain unqualified.

## Durable engine batch scheduling

```sh
pulsar stash generate-missing --mappings mappings.json --request-id BATCH_ID --detach
pulsar stash generate-missing --mappings mappings.json --scan --tag Pulsar-Generated --create-tag
pulsar stash batch-status BATCH_ID --offset 0
pulsar stash resume-batch BATCH_ID --request-id RESUME_ID
pulsar stash cancel-batch BATCH_ID --request-id CANCEL_ID
```

The default command waits for the engine batch and reports all selected outcomes. `--detach` returns a submission acknowledgement; it does not establish completion. Scan and tag effects are opt-in, and requesting a tag also enables scan. Creation still requires `--create-tag`. CLI and desktop now submit and observe the same engine-owned scheduler. Closing a client does not cancel the remaining phases.

The engine materializes discovery before mutations and saves each phase request ID, exact command, project/revision binding and outcome in its private batch ledger. It binds selected scene/file/server paths, snapshots an explicit model before scheduling, and retains the generation engine/tool identity. Capture, generation, commit, publication, scan and tag effects use existing authority operations. Every new phase still checks current project grants; publication and later effects use the recorded committed revision. Internal scheduling uses a scoped engine-memory permit bound to one exact request and executing thread, with no bearer credential or Stash key value in the ledger. A changed owner capability or generation engine/tool identity does not silently renew scheduling authority.

[Captured model identity](PULSAR_STASH_BATCH_MODEL_IDENTITY.md) is now checked after immutable preparation and before a job/attempt is published. A changed captured model cannot silently become the batch input. The exact original locator digest is bound to the private scoped permit; generation dependency mismatch retains the pending effect in `needs_attention`. Matching-byte restoration followed by explicit resume can continue the original phase. [Explicit future-generation migration](PULSAR_STASH_BATCH_MIGRATION.md) now accepts a full replacement model/configuration under current owner authority and an exact observed ledger digest, preserves old/accepted phases and leaves the batch paused until separate resume. No public permit, grant renewal or automatic cleanup is added.

On restart, running batches reconcile their recorded pending phases and successful authority receipts before continuing. They reuse the original effect IDs. Unknown publication/scan/tag dispatch is not repeated with a new ID. A recorded interrupted generation job can use the existing checkpoint-qualified resume operation once; no replacement job is silently created. Unresolved jobs or effects become `needs_attention` and retain IDs/artifacts. `resume-batch` continues the same ledger and rechecks authority; cancelled and finished batches cannot be resumed. A delayed tag creation may resolve by read-only lookup before the originally requested ADD is dispatched, provided ADD has never been dispatched.

Cancellation stops future scheduler effects, preserves committed work and best-effort cancels a known active generation job under current Generate authority. It does not roll back scripts or cancel a remote Stash scan. A phase already committed at its dispatch authorization point may complete; an in-flight uncommitted capture cannot acquire new authority after cancellation. Cancelled batches remain stopped after restart. Cancellation returns an acknowledgement without disclosing revoked project data.

Admission is bounded to two running batches, 100 selected scenes, a 16 MiB private ledger and reserved metadata memory. Each status response exposes at most five items under a bounded response size; `offset` selects another page. Read permission is checked for the projects in a returned page. The desktop provides next/previous page controls. Selection order and count remain stable across recovery. CLI waiting collects pages only after the batch reaches a terminal or attention state.

Internal phase IDs are reconciliation identifiers for the engine ledger. Their successful replay occurs through batch recovery; attempting to replay an internal phase as an ordinary client mutation does not bypass authentication or its original request identity. Standalone scan/tag status commands can inspect the recorded remote effects under current authority.

## Scan submission and reconciliation

```sh
pulsar stash submit-scan PROJECT_ID --publication-request-id PUBLICATION_REQUEST_ID --revision REVISION --request-id SCAN_REQUEST_ID
pulsar stash scan-status PROJECT_ID --scan-request-id SCAN_REQUEST_ID
pulsar stash reconcile-attachment PROJECT_ID --scan-request-id SCAN_REQUEST_ID
```

Publication receipts now include their request ID. Submission requires a successful, durable Stash publication from the same authenticated session/project, matching endpoint, current Export authority and expected revision. It independently verifies the captured source and published sibling bytes and re-queries the selected scene/file. It submits exactly one recorded server file path with `rescan: true`; optional derivative generation is disabled. It never submits empty paths or scans the entire library.

The engine commits a unique publication/scan dispatch record before sending the mutation. Its receipt distinguishes `submitted`, remote job states, `job_missing`, `finished_unattached`, `verified_linked` and `dispatch_unknown`. Transport errors and invalid/missing acknowledgements remain unknown because Stash may already have accepted the mutation. A new request cannot submit another scan for the same publication. Successful or unknown exact submission replay does not dispatch again. With a retained job ID, status can reconcile after engine restart. Without a job ID, an unknown dispatch remains unknown and needs external reconciliation; there is no blind retry or manual override yet.

`reconcile-attachment` explicitly observes the selected scene even when the scan job ID was lost. It performs only queries and local descriptor/hash verification under the original endpoint/session/project and current owner/Export authority. Its durable `attachment` field records a UTC observation time and `observed_linked`, `unattached`, `identity_changed` or `local_publication_unverified`. `observed_linked` requires the exact selected file ID/path, `interactive: true` and unchanged local captured/published bytes; it always declares `remote_script_hash_verified: false`. The observation does not recover a job ID, turn `dispatch_unknown` into scan completion, authorize tagging, or permit a duplicate scan. A negative observation invalidates an older verified-linked attachment status. Concurrent scan acknowledgement cannot erase the observation. Restart retains it, while a fresh observation can downgrade it if the scene, file or local bytes change. Desktop controls expose both scan state and the separately timestamped attachment observation.

`FINISHED` only becomes `verified_linked` after an independent scene re-query reports `interactive: true` for the exact selected file ID/path and local source/script bytes still match the publication. This verifies the observed attachment flag under the declared mapping; it does not verify a remote script hash, semantic script quality or release qualification. Missing jobs, failed/cancelled scans, changed files and finished-but-unattached scenes never become successful attachment. Status observations are persisted with compare-and-swap protection against older concurrent observations. Polling is explicit and bounded per call, rather than holding a client connection indefinitely.

The desktop offers submission and reconciliation for the current publication, retaining the scan request ID before queueing it. It also accepts a recorded scan ID for the current project. Both use the engine broker. [Actual authored v0.31.1 compatibility](PULSAR_REAL_STASH_COMPATIBILITY.md) now passes through the production CLI. Authenticated setups, user-library compatibility, other versions and interactive desktop usability remain unqualified.

## Tag creation, ADD and reconciliation

```sh
pulsar stash add-tag PROJECT_ID --scan-request-id SCAN_REQUEST_ID --tag Pulsar-Generated --create-tag --revision REVISION --request-id TAG_REQUEST_ID
pulsar stash tag-status PROJECT_ID --tag-request-id TAG_REQUEST_ID
pulsar stash resume-tag PROJECT_ID --tag-request-id TAG_REQUEST_ID --revision REVISION --request-id RESUME_REQUEST_ID
```

Tagging requires a recorded verified scan attachment in the same authenticated session/project, current local owner capability, Export authority and expected revision. Before dispatch, the engine independently rechecks the recorded scan job is FINISHED, exact scene/file attachment and local source/script bytes. It resolves one exact tag name with a bounded lookup; ambiguous results, case drift and identity changes are refused. `--create-tag` explicitly authorizes creation when the name is absent; otherwise absence fails without creation. Returned tag IDs are retained separately from scene attachment outcomes.

A durable unique scan/name dispatch record precedes creation or ADD. Creation acknowledgements are checked and independently re-queried. Lost creation acknowledgement remains `creation_unknown` until a later read-only lookup resolves it; creation is never repeated automatically. Scene attachment uses `bulkSceneUpdate` with `tag_ids.mode: ADD` for one scene and one tag, preserving existing relationships. An independent scene query must then observe the tag before the state becomes `verified_added`. Unknown ADD acknowledgement can be resolved by observation, but absence does not authorize a blind retry.

`tag-status` performs only remote queries. It distinguishes missing tag identity, removed scene relationships, unresolved creation, resolved-but-undispatched tags and unknown ADD outcomes. If delayed creation resolves to an exact tag ID and no ADD was ever dispatched, `resume-tag` may explicitly attach it under fresh authority/revision checks. Resume has its own request ID; the receipt retains the original tag operation ID. Concurrent phase checks prevent an older creation observation from replacing an ADD dispatch record or issuing a duplicate ADD. Successful exact effect replay returns the durable receipt without another mutation.

Desktop controls expose exact name, optional creation, retained tag/resume IDs and separate reconciliation. Tag success does not qualify the script or repair a failed scan. No actual user Stash instance was contacted. [Anonymous authored v0.31.1 compatibility](PULSAR_REAL_STASH_COMPATIBILITY.md) passes against an isolated real server; authenticated setups, the user library, other versions and interactive desktop usability remain unqualified.

## Remaining pipeline and verification

[Portable rebase ancestry](PULSAR_PORTABLE_REBASE_ANCESTRY.md) now preserves explicit unchanged-motion parents across capture/clone/re-export. Publication checks the parent and admission-base clock histories, rejects altered imported bases and supports rebased edits across a second clone. Format 9 and schema 11 protect the new records; legacy absent facts and differing histories remain unknown.

[Explicit archived Stash publication](PULSAR_ARCHIVED_STASH_PUBLICATION.md) now consumes current master ancestry and retained original clocks under fresh source binding and current authority. Proof schema 3 marks historical job identity explicitly; its first outbox advances the reader boundary to schema 10. Native proofs 1/2 and native-only schema 9 remain supported. Missing rebase and mixed-clock correspondence still refuse.

[Retained generation origin inspection](PULSAR_ARCHIVED_GENERATION_ORIGINS.md) verifies historical source/configuration/program facts and original placed clocks through exact clone namespaces. It uses retained object readers without opening old worker paths, creating a job or granting publication authority. [Current master ancestry](PULSAR_ARCHIVED_MASTER_ANCESTRY.md) verifies local edits and retained authored bases, imported retiming and effective undo restoration. Changed imported SQL correspondence and unrelated contributions refuse.

Explicit changed engine/model/owner migration is implemented for future unstarted generation; accepted jobs and uncertain external effects remain retained. [Portable source association retention and explicit fresh rebinding](PULSAR_PORTABLE_STASH_ASSOCIATIONS.md) are implemented locally. Recovery of irretrievably lost remote job identities remains unknown. Attachment observation without a job ID is implemented and remains separate from scan completion. Generation requests no remote mutations by default; optional batch scan/tag phases and standalone commands provide durable dispatch and reconciliation. Successful script export, scan completion and verified Stash attachment must remain separate outcomes. An unknown scan outcome must be reconciled before another scan is submitted. The legacy `auto-generate` operation remains unsupported rather than silently ignoring its scan/tag options.

[Upstream compatibility research](../research/phase-b/stash-integration-research-2026-09-29.md) pins these requirements. [Current validation](evidence/roadmap-2026-10-01/validation.json) includes offline HTTP and compiled CLI tests covering publication, replay, non-overwrite, refused motion gaps, scan restart reconciliation, unknown acknowledgements, remote states, attachment checks, tag-preserving ADD, uncertain creation/attachment recovery, query-only unknown-job attachment observations, concurrent scan acknowledgement/observation and concurrent creation/resume dispatch. No actual user Stash instance was contacted. [Real authored v0.31.1 compatibility](PULSAR_REAL_STASH_COMPATIBILITY.md) now has passing production CLI evidence; authenticated/user-library/version-matrix compatibility and full desktop usability remain unqualified.


## Native contributor receipt verification — 2026-10-09

[Contributor origin verification](PULSAR_CONTRIBUTOR_CLOCK_VERIFICATION.md) closes an actual CLI defect: the predecessor published an equal-clock merged script despite a corrupted contributor receipt. The bounded ancestry walk now verifies each contributing native origin and sample-plan arithmetic. Corrupt/missing receipts refuse before sibling creation; restoration permits valid publication. Default/CUDA Engine suites, offline release, architecture guards and both native CLI regressions pass. Per-action clock derivation and durable mixed-clock proofs remain pending; roadmap status remains PARTIAL.


## Exact merge action origins — 2026-10-09

[Exact merge action origins](PULSAR_EXACT_MERGE_ACTION_ORIGINS.md) adds an optional collector to the production merge kernel, retaining each output action’s exact Current/Candidate input and original index under the actual protection/write mask. Equal-valued boundaries remain Current. Core composition checks the correct source frame for an interior contributor and refuses a source-time collision. All core unit/integration targets and default/CUDA Engine regression suites pass. Engine ancestry composition, durable mixed-clock proofs and portable closure remain pending; roadmap status remains PARTIAL.


## Native authored-edit clock origins — 2026-10-09

[Native edit origin verification](PULSAR_NATIVE_EDIT_CLOCK_ORIGINS.md) closes a reproduced CLI defect: a corrupted authored-edit receipt previously still permitted publication. The ancestry walk now verifies receipt/upload/CAS identities and replays values-only reconciliation, including exact authored/inherited intervals. Corrupt/missing evidence refuses and restoration permits valid publication. Source-bound release and native regressions pass; an earlier worker-fence lease failure and incomplete serial attempt are retained, with concurrency root cause unresolved. Per-action ancestry composition and durable mixed-clock proofs remain pending; roadmap status remains PARTIAL.
