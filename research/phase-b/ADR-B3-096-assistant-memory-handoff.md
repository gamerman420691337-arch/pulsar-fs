# ADR B3-096: lend owned offline admission to authenticated assistant work

Decision: first attempt normal CPU assistant admission. If unavailable, require current Read/Edit/Generate authority and revision, validate owned generation input checkpoints and dependencies, then transactionally interrupt eligible jobs in the same session/project. Signal captured cancellation handles only after commit. Leave previews, live playback and device handles outside the victim set.

Wait for actual reservation release with a bounded cancellation-aware deadline; cancellation alone does not release custody. Transfer the admitted reservation into supervised assistant launch without releasing and reacquiring it. Publication remains fenced by job state and attempt identity.

After assistant completion, resume marked jobs through ordinary authenticated ResumeJob. The original job ID and immutable input boundary survive; computation restarts with a new attempt. Paused attempts do not consume the three-attempt failure budget, but all retained attempts remain bounded at 128 and the job counter must match history. Annotated validators and withdrawal recognize paused attempts.

Transient completion custody retries admission on subsequent observations. No session token is persisted. Existing boot recovery applies to CPU jobs; CUDA restart still requires fresh owner/native admission. This is not partial decoder-state recovery or physical playback qualification.

See [handoff contract](../../docs/PULSAR_ASSISTANT_MEMORY_HANDOFF.md).
