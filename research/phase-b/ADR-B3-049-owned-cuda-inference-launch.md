# ADR B3-049: Owned CUDA inference launch through shared supervision

Status: Implemented internal launch path; Engine GPU jobs remain disabled.

The borrowed CUDA research launcher cannot retain native owners through the shared retirement reaper, while ordinary production launch already implements bounded framed response/artifact egress and result checks. Duplicating those checks would create divergent acceptance and cleanup paths.

Bind measured admission to the exact request hash and consume its reservation, captured image and device pins. Assemble an owned inference mount command against a fence-admitted group, installing assets before fallible preparation. Extract the ordinary launch transport/retirement/result body as a shared supervisor accepting a serializable envelope. CPU supplies its request; CUDA supplies the verified mount witness. Close the child-mutex scope before pipe I/O. Keep existing artifact parsing, CUDA evidence checks and whole-tree retirement fencing.

A real authored CUDA inference fixture exercises this combined path, including original trace/receipt validation and expected positions; changed-request and pre-launch cancellation exercise release and refusal. The test uses a durable private fence, not a committed Engine GPU job. Cooperative accounting remains distinct from actual device quotas, and blocking host I/O remains an independent limitation.

No client protocol, database schema, portable wire format, inference algorithm or model weights change. Engine preparation, durable intent/acceptance/restart integration, Preview, model/media quality, platform, physical Handy and human acceptance remain open. Verification and exact subjects are recorded in [the scoped checkpoint](../../docs/evidence/roadmap-2026-10-07/owned-cuda-launch/checkpoint.json).
