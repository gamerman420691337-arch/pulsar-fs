# ADR B3-138: desktop assistant host settings

Status: implemented; headless input/controller and actual executable-host tests PASS. Native rendered GUI/platform/human acceptance remains UNKNOWN.

Persistent host policy is available through the trusted local executable, but the desktop cannot configure it. The application may also remain open when Engine startup fails, so recovery settings must remain usable without a project or healthy Engine.

Add a project-independent Settings tab using a typed host callback supplied only by the executable composition root. Client presentation never imports Engine/native runtime, writes host policy, sends configuration RPCs or starts/stops an Engine. Single pending off-thread work captures the exact approved request; inputs cannot change while pending. Bounded replies/errors and disconnect diagnostics permit explicit inspection after uncertain completion, never automatic replay.

Expose inspection of the saved private policy digest and validation state. Creating a measured policy and selecting it require explicit host inference approval; disabling requires the displayed digest and separate approval. Editing inputs clears approval. Saved settings affect subsequent startup only, while existing inference/project approvals and live authority remain separate. No default candidate, download, qualification or device authority is inferred.

Verify off-thread dispatch, approval/input binding, one pending request, reply validation, failure/disconnect recovery, headless UI pointer behavior and actual host callback configuration using owned temporary data. Native rendered GUI/platform/human acceptance remains an independent gate.

The host callback captures the configured Engine state/resource context at window composition; a pending operation cannot be rebound to a replacement callback. Safe corrupted saved bytes remain inspectable by digest so explicit disable can recover them. See [desktop settings evidence](../../docs/PULSAR_DESKTOP_ASSISTANT_SETTINGS.md).
