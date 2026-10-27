# Desktop image assistant preparation

Status: software increment implemented and tested; full roadmap PARTIAL; unqualified.

The local assistant panel now presents owner-registered text and vision profiles. A vision selection shows the current imported media catalog and a separate image choice. The catalog does not classify stills: the Engine checks exactly one frame during supervised preparation and rejects a video. The user must enable synthetic motion and choose its allowed channels, duration, sampling and amplitude. Target, edit and review focus remain exclusive with creative scope.

Saving a request normalizes the selected source through the existing Engine API and records the returned image context in a private intent. The shared CLI/desktop preparation module validates the draft before decoding, then checks the original source and exact project/session/revision before publication. The review shows source, dimensions and normalized content identity, explicitly distinguishing an image theme from observed temporal motion. Preparing and restoring do not run inference. Existing explicit inference and separate generation approvals remain required. Exact saved request and byte-digest checks remain in force for retries.

Changing projects or revisions clears the selected image. Refreshing sources removes choices no longer in the returned catalog. Old replies cannot replace the current view. Text selections omit an image, preserve the existing request encoding and keep ordinary target/edit/review flows.

## Verification boundary

The client library passes 256 tests with 5 ignored in the bounded, network-isolated RAM harness (7.44 seconds). Both strict workspace/all-target lint configurations and the final default application/worker build pass. Format, explicit own-file whitespace, 403 structural checks and 36 guard tests pass. The first test build failed with E0599 because the new fixture used the wrong command variant; the fixture now uses the actual StartAssistant variant, and the negative log is retained. Controller tests cover checked preparation, namespace/source refusal, no publication on failure, private restore and explicit inference approval. Headless egui shapes prove only that the presentation constructs a frame. Interactive visual QA is UNKNOWN: isolated Agent Workspace tools are unavailable in this session, and the selected [workspace skill](/home/brianklam/.codex/skills/agent-workspace-linux/SKILL.md) says, “Do not send input to or screenshot the user's real desktop.” No real desktop was controlled.

The preceding native Engine run verifies actual source normalization and vision inference with a measured private owner policy; it does not establish this desktop socket path, interactive GUI behavior or useful model quality. Earlier shape-to-pattern quality controls remain FAIL. No annotations, training or acceptance were fabricated.

## Remaining work

Dedicated image-conditioned candidate lineage, combined text/vision startup policy, offline first-launch model delivery, useful-model and real-reference acceptance, GUI/platform, Handy 2 Pro and human/release gates remain open. The roadmap remains incomplete.

Preceding setup and CLI contract: [vision policy and CLI](PULSAR_VISION_OWNER_POLICY_CLI.md).

Exact before/after sources, logs, binary identities and scope: [checkpoint](evidence/roadmap-2026-10-11/desktop-image-assistant/checkpoint.json).

Follow-up: [retained image generation origin](PULSAR_IMAGE_GENERATION_ORIGIN.md) implements scoped receipt/candidate/object-capture retention. Full portable lifecycle and presentation/qualification gates remain open; earlier evidence stays historical.
