# Native capture copy candidates

Three additional cold-copy candidates are rejected for production adoption. The current copy implementation, capture protocol ceiling, aggregate admission and software deadlines remain unchanged.

Each standalone diagnostic selects the same 81-library, 2,434,823,256-byte declaration used by the preceding shared-image handoff. It attempts FICLONE first, retains source metadata checks, synchronizes outputs/directories, checks every destination SHA-256 and repeats full final readback. Every successful output also receives independent Python size/hash verification before its owned directory is removed. These probes do not exercise complete native Ready or GPU preparation.

The isolated services use one CPU, no swap, 32 tasks and a 120-second diagnostic timeout. The default memory/address-space limits are 1 GiB/1536 MiB. Source files and the destination have different filesystem device identities in this execution environment; no SDK source files or shared services are changed.

| Candidate/order | Observed result |
| --- | --- |
| 8 KiB buffered / copy_file_range / copy_file_range / 8 KiB buffered | Buffered passes in 24.126/18.725 seconds. Both kernel-range attempts fail immediately with `Invalid cross-device link`. |
| 8 KiB buffered / sendfile / sendfile / 8 KiB buffered | All pass in 26.244/42.561/19.745/23.561 seconds respectively. No consistent sendfile advantage. |
| Unchanged buffered copy at 1/3/3/1 GiB memory ceilings | All pass in 17.464/22.999/41.471/23.046 seconds respectively. The 3 GiB cases additionally use 3584 MiB address-space limits. No consistent larger-budget advantage. |

FICLONE succeeds zero times. Ten complete outputs each pass independent readback of all 81 libraries. Both failed range-copy attempts and the initial warnings-as-errors indentation failure are retained; corrected diagnostic compilations pass without warnings.

Cgroup sampling retains memory, CPU and I/O counters. Both 3 GiB runs reach their larger memory ceiling and report max events without sampled OOM kills; the larger limit does not remove pressure or establish faster preparation. Process-level I/O/wchan sampling follows the timeout wrapper, so it cannot identify the actual copy child's wait site. Host cache and competing load are uncontrolled. These observations neither establish a latency distribution nor isolate a sole causal bottleneck.

The [packet](evidence/roadmap-2026-10-09/preview-artifact-open-race/checkpoint.json) retains sources, compile commands, per-service progress/cgroup samples, terminal states, comparisons and independent verification counts. All twelve owned services are terminal; only the two owned failed units are reset. Copy outputs and subsequent build/test scratch are removed. No kernel-copy or memory-budget candidate is adopted, and no lease is extended. Cold native readiness remains open.
