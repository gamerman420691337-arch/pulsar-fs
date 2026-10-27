# Live capture receipt replay through the CLI

Start, Continue and ContinueAt accept `--engine-epoch ORIGINAL_BOOT_ID` together with an explicit `--request-id ORIGINAL_REQUEST_ID`. Omit the boot option for normal admission using the current Engine boot. The shared client helper validates an explicitly declared boot and preserves it in the existing protocol command. The Engine decides exact replay versus admission using current authorization, revision and request fingerprint checks. Protocol and database formats are unchanged.

For a saved Start receipt, use the original paired session, project/revision, options, request ID and boot:

```sh
pulsar live-capture start PROJECT_ID 0 original-options.json --request-id ORIGINAL_REQUEST_ID --engine-epoch ORIGINAL_BOOT_ID
```

The same flags apply to `live-capture continue` and `live-capture continue-at`; preserve the original predecessor, options or source anchor, and lease. A changed payload conflicts. An unknown request with an old boot refuses. The explicit boot flag requires a request ID. Repeating an old request ID with the implicit new boot changes its fingerprint and refuses; it cannot silently start a replacement. Exact saved replay returns inactive EngineRestarted admission metadata without restoring pixels, a lease, registry, worker or current clock.

The new authored integration case crosses the shipped CLI, authenticated local IPC and actual confined CPU worker. It starts a capture, performs explicit and derived continuation, submits RGB pixels and completes one inference pair. It confirms SIGKILL of only its direct test-owned Engine, observes the exact recorded cgroups becoming empty without signaling/deleting them manually, then starts another owned Engine against the retained state. The Engine recovers its worker/staging journals before the client replays all three original requests as inactive history. Old status/intake, implicit changed-boot replay, changed options, missing explicit request identity and unknown old-boot admission refuse. A fresh Start mints new boot/capture/source identity, reaches Running and stops with empty ownership journals.

Independent readback verifies the three unchanged original admission rows, replay/fresh identity relationships, schema20, zero offline jobs/imported media, matching authored model/runtime identities and final empty custody records. Only authored inputs, public command reports and redacted admission rows are copied into the evidence packet; credentials/bootstrap/state files are excluded. The private original run root remains retained.

Final gates pass 1,032 workspace tests (55 ignored), the CLI grammar case, the default build, the new IPC/SIGKILL case and all nine preceding CPU application cases without skips, plus 265 structural checks. The pinned preceding CLI rejects the new boot option before creating Engine state. All 359 source records and both archives match the working tree. See [the checkpoint](evidence/roadmap-2026-10-07/live-capture-cli-replay/checkpoint.json) and [ADR B3-066](../research/phase-b/ADR-B3-066-live-capture-cli-replay.md).

This is selected CPU owner-death recovery evidence. CUDA-specific gates were not rerun in this increment; the preceding 549 CUDA tests and native graceful CUDA restart retain their separate source/binary identities. Actual CUDA owner death, power loss, startup/in-flight/cleanup-overlap handoff, finalizer races, sustained acquisition/performance, retained-evidence inspection/retention, known parallel failures and product/model/media/six-axis/Handy/platform/human/release qualification remain open. No private references, training or device actuation are used.

The subsequent [native CUDA CLI owner-death checkpoint](PULSAR_CUDA_CLI_RECOVERY.md) now covers a selected actual CUDA SIGKILL cut, inactive replay and fresh public Finish on unchanged production Rust source. Its evidence is separate from this CPU/CLI implementation checkpoint.
