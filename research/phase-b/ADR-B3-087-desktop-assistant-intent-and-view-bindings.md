# ADR B3-087: desktop assistant intent and view bindings

Status: implemented software increment; qualification incomplete.

The desktop previously exposed a disabled assistant placeholder despite an authenticated Engine and first-party CLI. The Doctor panel now discovers owner-registered profiles and saves a private exact start intent before separate inference approval. A disposable view never owns model or effect authority.

Use the existing private intent reader/writer and common SessionClient. Capture an intent digest for a prepared request, verify its current namespace and project/revision, then dispatch one owned checked value. Preserve original IDs for lost-acknowledgement reconciliation. Do not reload or substitute bytes between validation and dispatch, overwrite saved paths, automatically run inference or automatically delete uncertain state.

Use a separate bounded request/reply channel and background RPC thread. Bind each work item and reply to project/revision; discard stale replies and reset disposable approvals on view changes. Poll only visible Running state, stop automatic polling after errors and keep explicit cancellation independent from inference approval. Display earlier-revision attempts as historical results.

The model remains a proposal source. Protected action/history dispatch, current target/review/edit context, generation checkpoint/assistant/resume, private-intent lifecycle, default/offline packaging and rendered/native/learned/device acceptance remain independent work. See [desktop behavior and evidence](../../docs/PULSAR_ASSISTANT_DESKTOP.md).
