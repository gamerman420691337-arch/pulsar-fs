# Evidence panel worker failure

The Capture evidence panel now treats a disconnected background-worker channel as a terminal controller failure. It clears the busy flag, invalidates records and destination drafts, disables requests, and tells the user to reopen the desktop window. Because the worker might have published an already requested export before disconnecting, the message explicitly asks the user to check that destination before retrying. No successful export, cleanup or rollback is inferred from the disconnect.

Project navigation does not reactivate the stopped worker. A failed enqueue on a disconnected request channel follows the same terminal path; a full queue remains a retryable queue condition. Ordinary Engine errors are still returned through the live worker and do not mark the worker stopped.

Scoped software verification passed in [the failure checkpoint](evidence/roadmap-2026-10-08/desktop-evidence-worker-failure/checkpoint.json). The regression injects channel disconnection while the view is busy, checks stale-state invalidation, refuses further requests, changes revision and builds the disabled panel with headless egui.

Actual isolated-window QA remains unverified. The agent-workspace-linux skill requires orientation/permissions and `workspace_start` with `ack_hidden_workspace=true` and a purpose. No workspace-family tools are exposed in this session; no workspace was started and the host desktop was not accessed. This limitation does not block ordinary source implementation or its local checks. Native-window/human usability, retention controls and broader roadmap qualification remain open.

The final serial workspace gate passed 1,045 tests (56 ignored), including the worker-disconnection regression. The native desktop-worker positive case, shipped CPU-only CLI campaign, exact SQL/file readback and all 270 structural checks passed. All 367 source records match current files, snapshots and archives. Only the evidence panel source differs from the preceding desktop checkpoint.
