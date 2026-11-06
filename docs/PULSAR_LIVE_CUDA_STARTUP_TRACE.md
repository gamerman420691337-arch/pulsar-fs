# Live CUDA startup process trace, 2026-10-09

Status: diagnostic reproduction PASS; roadmap PARTIAL.

The [checkpoint](evidence/roadmap-2026-10-09/live-cuda-startup-trace/checkpoint.json) repeats the public authored RGB measurement under `strace -f -ttt -T -e trace=execve,exit_group`. Both epochs pass, and final original evidence verifies 179 runs and 537 CUDA-only computation nodes. Owned processes are terminal, worker custody empty, and no offline jobs exist. Source/executable identities match the preceding 407-record measurement packet.

| Observed process interval | Start epoch | Continue epoch |
| --- | ---: | ---: |
| Native dependency capture process | 2,815.38 ms | 2,986.56 ms |
| Memory observer process, including re-exec | 5,970.90 ms | 5,646.44 ms |
| Observer initial exec to loader re-exec | 1,954.83 ms | 1,876.16 ms |
| Inference worker initial exec to loader re-exec | 4,002.45 ms | 3,800.70 ms |
| Overall command initiation to observed Running | 18,700.00 ms | 17,743.98 ms |

Nested intervals must not be added together. Process termination timestamps include retirement, and these intervals include all work in each stage; they do not isolate pure hashing, copying or driver initialization. Tracing perturbs timing: the untraced 13.94/13.67-second baseline remains the appropriate development-build baseline.

Current source explains what precedes readiness. `engine_live_cuda.rs` captures native dependencies, prepares immutable worker/model snapshots, observes and admits GPU memory, then launches the private stream. `live_cuda_mounts.rs` verifies mounted bytes before loading. `worker/live_cuda_stream.rs` checks worker/model bytes before and after loader re-exec and after model initialization, before emitting Ready. The pinned native image declares 75 libraries totaling 1,945,994,000 bytes. Byte checks, deadline/cancellation checks and ownership must remain effective during performance work.

The measured executable is a 631,241,945-byte development build. A release comparison is the next experiment; its build and results are separate evidence and are not claimed here. This trace demonstrates substantial pre-inference preparation time, not a proven single root cause or an accepted optimization. Only selected process roles, timestamps and terminal markers are published; the original exec trace remains privately retained with a checksum. No full environment or command material is published.

No source changes or security relaxation occurred. This is short authored scalar RGB coverage, excluding camera acquisition, encoded decoding, live six-axis reconstruction and dispatch. It establishes no 200 ms capture-to-command, useful model quality, platform, physical Handy or release qualification. Earlier worker-fence lease intermittence remains open.
