# Staging cleanup and storage-delay diagnosis — 2026-10-09

Status: PARTIAL. The preceding default-suite staging-cleanup failure remains unresolved. No production cleanup rule or deadline was changed.

The [diagnostic packet](evidence/roadmap-2026-10-09/cleanup-io-diagnosis/checkpoint.json) investigates the unchanged `worker_fence::tests::format3_creator_retires_exact_obligation_and_releases_claim` path. The exact test passed once with syscall tracing, then passed ten consecutive isolated traced runs. These checks demonstrate current successful retirement, ledger clearing and claim release; they do not erase the earlier failure or prove reliability under all concurrent workloads.

Three hypotheses were considered: slow durability sync under host I/O pressure, repeated journal/identity failure, and delayed cleanup-thread scheduling. The fresh compiler was independently observed in `D` state at `balance_dirty_pages_ratelimited_flags`; host full I/O pressure reached roughly 92 percent over ten seconds. Core and protocol compiles took 62.42 and 114.85 seconds. The first direct Engine compile lacked CARGO_MANIFEST_DIR and failed; the corrected build passed. Both command receipts are retained.

The original concurrent Engine workload was then run with only fsync/fdatasync tracing. It hit the 240-second inner timeout; the terminal harness receipt reports exit 124 at 249.06 seconds. Its incomplete output records 115 passes and one failure in `imported_originals_survive_new_local_edit_reexport_and_second_clone`, without the terminal panic report. This is neither a completed suite nor a reproduced trace of the exact staging-cleanup failure. No retry of the live run was launched after observation timeouts.

The concurrent trace contains 434 successful sync calls exceeding three seconds, with a longest recorded call of 25.81 seconds. Owned test threads were observed at `jbd2_log_wait_commit`, `rq_qos_wait`, and `folio_wait_bit_common`. This supports storage delay as a contributor to current runtime instability, but instrumentation affects timing and the exact original cleanup failure was not captured. It is not an uninstrumented performance benchmark or proof that cleanup logic is correct.

All 422 production/source inventory records remain identical to the CUDA producer-identity checkpoint. No source edits, release rebuild, model training, private media access, device actuation or shared-service changes occurred. Scratch compiler outputs, test databases and traces were removed after copying diagnostic evidence and confirming no visible owned process references. Shared caches and services were preserved.

Next work remains full mixed-clock publication and portable correspondence, cleanup/cold-start reliability with stronger uninstrumented fault evidence, real-media/model qualification, Handy 2 Pro credentials/firmware and physical acceptance, and platform/release gates. Full roadmap remains PARTIAL.
