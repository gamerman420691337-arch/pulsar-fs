# ADR B3-066: Preserve the original boot in explicit CLI receipt replay

Status: Implemented and locally verified. Broader recovery and qualification remain pending.

Allow an explicit Engine boot ID on the three existing capture admission commands, requiring an explicit request ID. Preserve normal current-boot discovery when omitted. Share boot resolution through EngineApi without importing Engine implementation or adding wire/database formats. Existing Engine authority decides exact saved replay, changed-intent conflict or stale/new admission.

Verify the original fingerprint through shipped CLI/local IPC after confirmed death of an owned active Engine. Observe the exact recorded groups becoming empty before recovery; never replace that observation with manual group deletion or signaling. Separate CPU SIGKILL evidence from CUDA owner death, power loss, source continuity and physical qualification.

See [implementation, evidence and limits](../../docs/PULSAR_LIVE_CAPTURE_CLI_REPLAY.md).
