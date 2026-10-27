# ADR B3-142: desktop repair proposals

Status: implemented with scoped headless/native-controller evidence; rendered desktop acceptance and numeric/device qualification remain open.

Add repair controls to Doctor using the existing explicit repair-import command and Core neutral-grid policy. Import is an uncommitted sanitized candidate; the user reviews/commits its baseline through existing candidate controls before repair. Repair captures the immutable committed program, project, revision and checked policy, runs Core math on the existing RPC thread and uploads through the edit broker. It never commits or exports. Existing candidate review, revision/protection guards and history remain authoritative.

Disable duplicate/pending requests and policy inputs while busy. Invalid policy, absent project/master and unsupported Core input refuse. Validate returned candidate scope, base and values; evidence reconciliation belongs to Engine. Bind queued repair replies to captured project/revision and discard a result if the current view changed, retaining its candidate ID for manual inspection. No automatic replay, rebase, commit, source overwrite or protection override.

Verify drawn controls/input binding, off-thread proposal dispatch, absent/mismatched replies and no commit, then actual native import/proposal/master invariance/review/commit/history/stale/protected behavior against Engine. Headless egui and native controller evidence do not establish rendered desktop/human acceptance. Preserve all independent model/media/device/platform/release gates.

Verification: four headless panel tests and two RPC tests pass; full Clients/Engine suites pass 316/841 with 8/70 ignored. Actual network-disabled Engine/controller checks confirm uncommitted import/proposal, four independently expected values, explicit commit, stale/protected refusals and Undo/Redo. Both strict lint configurations, workspace check, formatting, native App/Worker/controller builds, 443 architecture checks and 36 source guards pass. See [desktop repair scope](../../docs/PULSAR_DESKTOP_SCRIPT_REPAIR.md).
