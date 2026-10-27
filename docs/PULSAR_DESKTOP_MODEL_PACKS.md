# Desktop owner-selected model packs

The Generator now has an **Offline model packs** panel using the same engine operations as the CLI. Enter a pack directory to import inert data, enter its manifest digest to check current installed bytes, and refresh the recorded owner selection. Choose **Owner-selected offline pack** under declared model analysis to use that exact selection for a stroke candidate.

Selecting a pack requires the explicit unqualified-execution checkbox and a refreshed selection revision. Changing the digest clears that approval and its displayed integrity receipt. Controls refuse selection while another request is pending. Deactivation changes future offline analysis and generation; accepted jobs retain their captured inputs. Each operation uses the paired session and separate host-local model-pack owner capability through the existing asynchronous desktop RPC queue. The presentation does not load model/runtime assets.

## Current status and recovery

A mutating response is an operation receipt, including an exact historical replay. It cannot become the current generation choice. The desktop automatically submits a separate current-status query after import, selection, deactivation or replay. Until that succeeds, generation from a pack refuses locally. A failed or lost acknowledgement invalidates cached selection while retaining the operation's original recovery ID and intent.

[Persistent session-bound recovery](PULSAR_DESKTOP_PACK_RECOVERY.md) now writes each original directory/digest/revision and request ID before dispatch. Restored entries support **Replay retained pack operation** even after window closure or form edits. New actions create new IDs. Only acknowledged entries can be explicitly dismissed. Restoration or an old successful receipt cannot populate current selection or reactivate it.

An integrity receipt identifies its exact digest. Owner selection is a recorded approval, not a fresh filesystem or quality result. Generation performs fresh engine-side byte/profile/selection checks and captures the immutable dependencies described in [owner-selected generation](PULSAR_SELECTED_MODEL_PACKS.md).

## Generation and remaining integration

The dedicated pack-generation command retains the current media source, preset, selected placement, project interval, observed-target declaration, video view and independent audio settings. The declared pack profile/input must agree with any caller declaration. Existing restrictions remain: for example, a manual target seed with reverse source placement refuses rather than weakening the generation contract. Pack generation never substitutes a loose model path.

[Pack-bound detector preview](PULSAR_MODEL_PACK_PREVIEW.md) now uses exact selection/model/profile and frame/view/time association. Temporal script-position profiles still need a frame pair and show pixels-only preview. [Stash batches](PULSAR_STASH_MODEL_PACKS.md) now submit the same exact pack choice/profile through their own durable admission and migration contract. These boundaries are visible in the interface.

## Local evidence

The [receipt](evidence/roadmap-2026-10-01/desktop-pack-selection/validation.json) records the final source and gates. Three headless Rust cases execute the actual egui draw/pointer/keyboard controls, verify disabled selection without current status/approval or while busy, edit the digest to clear approval, emit explicit select/deactivate actions, retain the exact replay intent after a lost acknowledgement, distinguish historical/current status, and compare all preserved generation settings. The rebuilt native CLI/engine/worker campaign remains a separate integration check; it does not execute a native desktop window.

The isolated-workspace backend is not exposed in this session. No native-window screenshot, mouse interaction, rendered usability, cross-platform desktop or human acceptance claim is made. Trusted publisher/update policy, distribution, learned-model quality, installed Stash, Handy and the full roadmap remain open.
