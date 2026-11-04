# Public live CUDA measurement, 2026-10-09

Status: mechanism verification PASS; roadmap PARTIAL; capture-to-command qualification remains open.

The [checkpoint](evidence/roadmap-2026-10-09/live-cuda-measurement/checkpoint.json) records two authored RGB epochs through the public shipped CLI. Each uploads 60 frames paced at 30 fps, then 120 frames without waiting for inference completion. Explicit Continue creates the second epoch with advancing source time. Only the final epoch consumes Finish and retains original final CUDA evidence; first-epoch observations remain provisional.

| Measurement | Start epoch | Continue epoch |
| --- | ---: | ---: |
| Start/Continue command return | 10.82 ms | 6.36 ms |
| Command initiation to observed Running | 13,941.76 ms | 13,667.36 ms |
| Paced 60-frame phase | 1,987.86 ms | 1,985.90 ms |
| 120-frame upload burst | 823.87 ms | 814.10 ms |
| Distinct observed inference reports | 167 | 168 |
| Observed probe-to-inference age range | 5.87–55.84 ms | 5.89–80.14 ms |
| Completed / accepted / stale pairs | 179 / 179 / 0 | 179 / 179 / 0 |

These are descriptive observations from a short authored run, not statistical acceptance results. Pending and in-flight queues stay at most one frame each, drain empty, and report one pending replacement per epoch. The completed count is 179 for 180 input frames; replacement counters alone do not identify lost inference pairs. The clock probe precedes RGB creation/upload, so the reported age conservatively includes that work. It is not a camera capture timestamp. Polling observes a subset of completed results.

The authored 302-byte scalar model computes `0.25 + 0.5 × mean`. All 360 upload headers match regenerated RGB hashes and advancing source timestamps. Observed pair positions match that formula within a maximum difference of 9.536743161842054e-8. This is a fixture oracle, not model quality evidence. Final original manifest/receipt/trace hashes match the public evidence descriptors; the trace contains 179 runs and 537 CUDA-only nodes, bound to the final capture origin. No offline jobs were created. Owned processes are terminal and worker custody empty.

The initial run failed because the test incorrectly expected raw mean. Its source, log and commands are retained under `initial-failure/`. Cleanup originally left recorded custody; a fresh owned Engine recovered that specific failed state and confirmed empty custody before exit. The corrected test waits for custody retirement before terminating its Engine. No manual group deletion was used.

The packet binds 407 source records: 406 unchanged from the preceding desktop pack checkpoint and one new measurement script. Production Rust and the executable remain unchanged. Current syntax, one native test with no skips, 288 architecture checks and 36 source guards pass. Prior Rust gates are inherited, and earlier worker-fence lease intermittence remains unresolved.

LIVE-02 defines 200 ms capture-to-command including decoding, inference and dispatch. This run excludes camera acquisition, encoded decoding, six-axis reconstruction and device dispatch; it cannot qualify that requirement. LIVE-09's statistical latency acceptance rule remains unqualified. Desktop seek waits are a separate responsiveness measurement. Roughly 14-second readiness gaps need further stage measurements; asynchronous preparation is the next investigation, not a proven root cause. Persistent model-run performance, long-duration behavior, useful model quality, real annotations, mixed clocks, installed Stash, platform/offline packaging, physical Handy and human/release acceptance remain open.

Reproduce with `node --test scripts/live-cuda-measurement.test.mjs` and explicit `PULSAR_CUDA_CLI_WORKER`, `PULSAR_CUDA_CLI_RUNTIME`, `PULSAR_CUDA_CLI_MODEL`, `PULSAR_CUDA_CLI_PROFILE` and `PULSAR_CUDA_CLI_POLICY` paths. Missing inputs skip the native test. This campaign used the identities retained in the packet; no training, live/sterile input, user service contact or physical playback occurred.
