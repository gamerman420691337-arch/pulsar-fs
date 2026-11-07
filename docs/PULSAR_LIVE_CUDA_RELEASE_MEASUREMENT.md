# Release live CUDA measurement, 2026-10-09

Status: release build and authored mechanism test PASS; roadmap PARTIAL.

The [checkpoint](evidence/roadmap-2026-10-09/live-cuda-release-measurement/checkpoint.json) binds the unchanged 407-record source inventory and a new release executable. `cargo build --release --features cuda` passes with existing warnings. The executable is 34,897,376 bytes, compared with 631,241,945 bytes for the preceding development build. No production code, build profile or verification policy was changed.

The same untraced public-CLI measurement passes with the same model, runtime, owner policy, GPU and two-epoch workload. Each epoch uploads 60 authored RGB frames paced at 30 fps, then 120 without waiting for completion. Explicit continuation advances source time. Only the final epoch consumes Finish and retains final original CUDA evidence; first-epoch reports remain provisional.

| Measurement | Start epoch | Continue epoch |
| --- | ---: | ---: |
| Command initiation to observed Running | 12,859.30 ms | 12,596.94 ms |
| Paced phase | 1,982.70 ms | 1,983.99 ms |
| 120-frame upload burst | 625.46 ms | 592.80 ms |
| Distinct observed reports | 166 | 168 |
| Observed probe-to-inference age range | 4.99–54.81 ms | 4.86–51.31 ms |
| Completed / accepted / stale pairs | 174 / 174 / 0 | 173 / 173 / 0 |
| Pending replacement counter | 6 | 7 |

The release executable does not remove the readiness gap. These are separate short runs, not a controlled statistical performance estimate. Faster burst ingress and different pending replacements mean completed counts cannot be compared as if the realized workloads were identical. Pending and in-flight queues remain at most one each and drain empty; no expired frames are reported. No claim of lossless burst processing is made.

All 360 input headers match regenerated RGB hashes and advancing source timestamps. Observed positions match the authored `0.25 + 0.5 × mean` oracle within 9.536743161842054e-8. The final original manifest/receipt/trace matches public SHA/length descriptors and the capture origin. Final evidence contains 173 runs and 519 CUDA-only nodes. No offline jobs were created; owned processes are terminal and worker custody empty. Source/archive readback passes; earlier architecture, guard and Rust gates are inherited on unchanged source. Earlier worker-fence lease intermittence remains unresolved.

The [development startup trace](PULSAR_LIVE_CUDA_STARTUP_TRACE.md) directs the next investigation toward preparation and repeated mounted-byte verification. The native declaration has 75 libraries totaling 1,945,994,000 bytes; their 84 declared aliases account for 3,109,940,952 bytes per full alias verification pass. That is a source-derived workload count, not a measured hashing duration. Any optimization must preserve exact mounted identities, fresh byte proof, cancellation/deadlines, aggregate resource accounting and ownership. No verification checks were removed in this experiment.

LIVE-02 capture-to-command and LIVE-09 statistical latency acceptance remain unqualified. This test starts from authored RGB and excludes acquisition, encoded decoding, six-axis reconstruction and dispatch. The observed report subset is not every completed-result latency. Long-duration live behavior, useful quality/real annotations, remaining desktop combinations, mixed clocks, installed Stash, platform/offline distribution, physical Handy and human/release gates remain open. No training, live/sterile input, user service contact or physical playback occurred.
