# ADR-B3-123: desktop synthetic proposal review and recovery

Status: accepted software contract; rendered GUI and broader qualification remain open.

The desktop assistant could restore creative generation declarations produced by the CLI, but could not prepare a creative Start or save a retained synthetic proposal itself. Add an explicit synthetic mode with selected channels, duration, sampling and amplitude bounds. Validate the host scope through the shared protocol/Core rules before enabling save. Empty or duplicate channels, invalid numbers and aggregate action-budget violations refuse; mixed target/edit/review focus remains forbidden.

Use existing private Start preparation and inference approval. Display the saved scope independently of the live controls. Retained completed synthetic proposals can save an exact private GenerationIntent without a media source. Require current owner/project/revision and read current project state before saving. Display exact pattern parameters and use the existing separate generation approval, restore, reviewed-file digest, acknowledgement recovery and candidate/Undo/Redo workflow. Pending generation retries block competing preparation and mutation.

The native probe executes the actual desktop controller against an owned Linux Engine with the pinned learned model/runtime and confined worker. It restores both declarations, refuses absent approvals, recovers the same generation job and yields a triangle independently checked against 17 numerical samples and exported amplitudes. Candidate commit and Undo/Redo are separately verified. Headless egui construction tests exercise creative controls without granting inference/generation authority. They do not establish rendered GUI usability or broad prompt adherence.

See [desktop capability and evidence](../../docs/PULSAR_CREATIVE_DESKTOP.md). Unavailable-axis model intent failure, model selection, offline bundles, platform/media/device and human qualification remain open. No milestone closes.
