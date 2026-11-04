# Saved assistant generation approval and desktop controls

First-party clients can now save and explicitly approve a complete assistant generation request. Desktop controls select an imported media source, preset, complete optional settings and model path. A separately approved completed stride proposal supplies the effective sample stride; a [target proposal](PULSAR_ASSISTANT_TARGET_APPLICATION.md) derives the checked seed and captured view. An accepted job enters the existing desktop job, candidate review and protected commit flow. Saving the declaration does not start inference, generation, commit or device playback.

## Exact private declaration

`GenerationIntent` has its own strict format: endpoint, session, credential namespace digest, original request ID, project, base revision, complete `GenerateWithAssistant` command and displayed proposed stride. Only this command family is allowed. The displayed stride must match the approval's proposal digest. Request validation checks the settings and ordinary protocol constraints. No session token or API key is stored.

Start and generation declarations share the existing descriptor-based Unix private file store. It creates a new owned mode-0600 file, refuses overwrite/symlink/hardlink/nonprivate/excessive input, admits at most32KiB, observes metadata consistency during capture and syncs publication. Windows private-file support remains unavailable; portability is not closed by these Unix checks. Loaded declarations are validated before use.

The CLI adds `pulsar assistant prepare-generation PROJECT ATTEMPT --source SOURCE --revision REV --request-id ORIGINAL --output /absolute/private/request.json`, with optional `--preset`, `--settings` and `--model`. It reads the completed owned report and captures settings into the declaration. Later `pulsar assistant generate-saved --intent /absolute/private/request.json --approve-generation` dispatches those captured options with the original ID. It does not reread a settings file or substitute a report, source, profile or model path. The saved declaration freezes options and paths; the Engine captures and validates model bytes through its existing dispatch path. Existing immediate `assistant generate` remains available.

## Desktop review and recovery

The desktop refreshes current imported Media sources and checks project/revision/source kind before saving. It displays the full saved command and effective proposed stride before a separate generation approval checkbox. Restoring a file resets approval. Start/retry reloads and validates the file, checks endpoint/session/credential/project binding and compares the canonical semantic digest captured at review. Editing other UI fields cannot redirect that saved request. Changed files refuse dispatch. Reading lists/status/explanations preserves pending mutation identity; a different generation or assistant mutation cannot displace a pending generation request.

Exact historical retries keep the saved base revision. The Engine's existing authenticated replay can recover an accepted original receipt before fresh revision/context validation; an unaccepted stale request refuses a new job. This client logic adds no new Engine grants or replay exceptions. Fresh generation still rechecks approval context/ownership/Engine lifetime and Read/Edit/Generate authority, captures sources/models and publishes through existing admission and transaction rules. Candidate commit, protection and undo remain separate existing actions.

Accepted current-view jobs are delivered once to the existing desktop job controls, which poll and offer candidate review. The assistant panel also offers a checked saved-job status read, including historical jobs. Wrong-view/job responses cannot replace displayed job metadata. Read-only status does not clear an uncertain generation mutation. Changing views clears disposable approval/cache state; durable saved files remain available for explicit restore.

## Boundaries and evidence

[The checkpoint](evidence/roadmap-2026-10-10/assistant-generation-intents/checkpoint.json) retains exact source, checks and runtime/build outcomes. Private test declarations use synthetic reports and recording clients with acknowledgement loss; they prove client identity/capture/dispatch and reply handling, not actual learned-to-generation effects or native desktop rendering. Earlier Engine approval/job tests remain independently bound evidence. New client controls do not qualify packaging, installed defaults, ordinary filesystem durability, model/media accuracy or hardware.

The full roadmap remains incomplete. Target selection, protected assistant edit recipes, checkpoint/live playback/resume, full private cancellation/withdrawal/approval lifecycle, bundled offline model/default delivery, annotations and evaluation, platform/native UI, Handy firmware/credentials/stopping and human/release gates remain open.
