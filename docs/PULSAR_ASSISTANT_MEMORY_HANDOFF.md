# Assistant memory handoff

Status: PARTIAL full roadmap progress; qualification INCOMPLETE.

Authenticated assistant Start preflights the owner-issued worker/runtime/model assets and first requests normal CPU admission. If memory is unavailable, current Generate authority is required before eligible queued/running generation jobs in the same session and project can pause. Up to 16 bounded manifests are inspected; their dependencies and exact initial checkpoints must validate. Previews, live/device handles and other owners are excluded.

The pause transaction records an interrupted job and a private paused-attempt state. Only a committed transaction signals its captured cancellation handle. Running jobs without an exact handle refuse. Output publication checks current state and attempt identity. The assistant waits up to five seconds, bounded further by its worker timeout, for actual accounting custody to retire. Its reservation then transfers directly into the ordinary supervised launch.

Completion resumes marked jobs through authenticated ResumeJob and ordinary worker admission. If resources remain occupied, a later assistant observation retries resume without rerunning inference. The same job ID receives a new attempt using the immutable input checkpoint; generation starts again, rather than recovering partial decoder progress. Failure retry accounting excludes intentional pauses, checks retained-history integrity, and caps all retained attempts at 128. Annotated database validation and model-pack withdrawal recognize paused history.

No credentials are persisted for this handoff. CPU boot recovery retains its existing behavior; CUDA recovery requires fresh owner/native admission. A revoked grant or unavailable resource can leave a recoverable interrupted job. No useful training annotations, learned quality, GPU/native live continuity, actual desktop contention, Handy synchronization or physical acceptance is implied.

Authored private tmpfs checks pass 27 assistant tests (one learned-runtime case ignored), nine annotated database tests, 12 resource tests and four assistant-launch tests (three native cases ignored). Separate ordinary-storage checks pass the handoff reservation/restart fixture and exact-handle retirement fixture; rollback/authority/checkpoint testing times out at 90 seconds on ordinary storage without an assertion verdict. The private passing result does not repair that timeout. The application/worker build, format, 375 structural architecture checks and 36 source guards pass. Final lint outcomes and retained source subjects are recorded in the [evidence checkpoint](evidence/roadmap-2026-10-10/assistant-memory-handoff/checkpoint.json).

Synthetic retained reports, owned input fixtures, inert workers and manual accounting retirement establish the orchestration contract. They do not establish actual learned-runtime contention, physical worker/GPU retirement, native live playback continuity, or partial-progress recovery. Earlier broad-suite failures remain open; the broad workspace suite was not rerun for this increment.


A later [native CPU contention case](PULSAR_NATIVE_ASSISTANT_HANDOFF.md) separately passes actual pinned generation/assistant execution and automatic same-job recovery from the unchanged initial checkpoint basis. It does not upgrade the earlier fixture subjects or prove pre-pause inference, partial progress, aggregate memory or live-device continuity.
