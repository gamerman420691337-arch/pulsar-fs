# ADR B3-131: Authenticated prepared-image assistant requests

Status: software increment implemented and tested; roadmap PARTIAL; unqualified.

## Decision

Expose an explicit revision-checked `PrepareAssistantImage` request for an imported media source. Reuse the supervised Preview worker with a private still-only mode: exactly one decoded frame, first frame at elapsed time zero, no detector, view, placement or annotations, packed RGB8, and at most 512 pixels per axis. Native media parsing remains confined to the worker.

Issue a pathless context binding project, session, revision, source version, original and derived artifact identities, actual ffmpeg/ffprobe identities and preprocessing version. The image identifier includes the unique attempt identity. Keep at most 32 successful contexts in the running Engine. Reserve the full output ceiling before decoding. Failed deletion keeps the directory and claim in a retry queue; new preparation refuses until cleanup succeeds. The catalog is ephemeral: restart, eviction or changed bytes requires preparation again. Crash recovery of orphaned attempt directories remains a separate durability gate.

Trusted host registration declares a vision projector on the assistant profile. The client selects an exact registered profile and Engine-issued image context; it cannot supply a pixel or projector path. High-level image requests require exclusively creative synthetic scope. Source membership, ownership, revision, original bytes and derived pixels are checked before launch, on completion and at creative generation approval. The worker response must echo the exact projector/pixel pair and dimensions. The saved report retains the context, and the creative prompt labels the image as a theme rather than observed temporal motion.

Absent optional fields preserve the text request encoding. Existing text registration and text execution remain supported. Generation approval continues to use its original request as the receipt and fingerprint subject; the proposal is separately checked against fresh context before its constrained patterns become a candidate job.

## Process CPU accounting

The full native request exposed a shared confinement defect: a two-thread, 120-second wall request inherited a hard process CPU limit of 122 seconds. Linux counts CPU across threads. The runtime was killed near this limit, with zero OOM counters and approximately 770 MB peak memory. Derive the process CPU ceiling from rounded-up wall seconds times declared threads, plus the existing margin. Keep aggregate concurrency and wall deadlines enforced independently by the existing cgroup and supervisor. The original request passes after the correction, with sampled CPU time above the old ceiling and no OOM counters. The original request is the native regression seam; rounding and arithmetic boundaries have a separate unit check.

## Limits

This increment does not provide an owner policy setup option, CLI/desktop image picker, dedicated image-conditioned generation lineage, durable pixel catalog, useful-model acceptance, real-reference annotations, physical playback qualification or portable release evidence. Existing prompt-only image generation is not silently upgraded by this request. Still imagery does not establish temporal frequency or faithful six-axis reconstruction.

No annotations or training are fabricated. The preceding candidate's shape-to-pattern instruction controls remain FAIL. Native request-path evidence and candidate quality are separate gates.

Evidence: [prepared-image implementation record](../../docs/PULSAR_AUTHENTICATED_IMAGE_ASSISTANT.md).
