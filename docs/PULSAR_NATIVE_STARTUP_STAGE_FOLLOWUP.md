# Native startup stage follow-up, 2026-10-09

Status: diagnostic evidence retained; full roadmap PARTIAL. No production source changed in this follow-up.

Two serial native campaigns use the exact adaptive-preparation release (SHA-256 `aabfb50de549f662fb1d6b829ce663a1a65ec10e26773349c66ba610cbe9cdda`) and unchanged 415-record source. Both expire before Ready, at observed 30.18 and 30.17 seconds. Reuse and successor output assertions are not reached.

| Sampled stage | Initial run | Repeat run |
| --- | ---: | ---: |
| Native capture last observed | 25.80 s | 19.98 s |
| Memory observer initial executable observed | 27.85 s | 22.13 s |
| Inference worker initial executable observed | not sampled | 26.41–29.60 s |

Sampling does not establish exact stage durations. Loader re-exec can remove the sampler's executable-role label. The first run shows native copying/verification followed by memory observation near expiry; the repeat reaches the inference worker but still expires. Startup failure is not invariably expiry inside library copying. Both native workers wrote the complete 1,945,994,000-byte library image, with 237,579 write syscalls at the retained sampled boundary. Neither observation establishes readiness or that every mounted-byte verification passed.

The first run's observed memory-observer cgroup reports memory.max 1,610,612,736 bytes, zero memory.max/OOM events, and zero sampled memory-pressure totals. This differs from the capture cgroup, which enforces the selected 1 GiB ceiling. No claim that observer memory pressure caused this failure is supported.

A standalone copy probe compared a forwarding Write implementation (matching the NamedTempFile forwarding copy shape) against a 1 MiB BufWriter around that same writer. It copied the actual 621,242,784-byte `/usr/lib/libonnxruntime_providers_cuda.so`, checked exact copied length, flushed buffering, synchronized the output file, and independently SHA-256 checked each output against the original. Serial order was direct, buffered, buffered, direct:

| Mode | Write calls | Copy + flush + fsync seconds |
| --- | ---: | ---: |
| direct | 75,836 | 4.430 |
| buffered | 593 | 31.725 |
| buffered | 593 | 28.421 |
| direct | 75,836 | 3.692 |

All four copies return zero and independently match source SHA-256. Hashing time is outside the tabulated interval. This is a standalone one-library probe; it excludes tempfile publication, final assembled image verification, cgroup/worker confinement, GPU observation and inference startup. Host load/cache is not controlled. Buffering reduces write calls but is slower in both tested orderings, so no production buffering change is justified or applied. It is not evidence for a general causal diagnosis or a tuned buffer optimum.

Both live final custody ledgers are empty and owned Engine exits are terminal SIGTERM. The retained packet under `evidence/roadmap-2026-10-09/native-preparation-budget/` includes repeat logs, observations, stage summary and exact probe source/executable/results. Disposable run roots, probe outputs and scratch executable are removed after retention. Release, real-media/model, Stash, platform/offline, physical Handy and human gates remain open.
