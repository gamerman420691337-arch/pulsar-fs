# Native mixed-clock runtime follow-up

Status: **PARTIAL / unqualified**. No production behavior or durability deadline was changed.

The unchanged mixed-clock fixture now reached a terminal failure after 193.26 seconds: `DependencyMismatch: invalid motion: selected clocks require bounded complete gap-resolved motion`. Its placement extended to source 875,000,001 ns, although stride three last decoded source frame six at 750,000,000 ns. The planner correctly retained an uncovered trim boundary, so the export guard correctly refused. This is a fixture precondition failure, not evidence of a clock-mapping defect.

The fixture now ends at source 750,000,001 ns, giving both strides a shared supported terminal sample and one project nanosecond of coverage. A new assertion requires each actual decoded proposal to have no gaps. Only `authority.rs` test setup changed; all other 421 of the 422 source records are unchanged. Default Engine compilation passed. CUDA, broad suites, and release packaging were not rerun after this test-only change.

The corrected run timed out at 480 seconds (482.84 seconds observed) with no completed export verdict. Samples show a test thread in filesystem journal waits during token/database setup and later while storing the stride-three actual producer output. The stride-three gap assertion was reached without failure; stride-one, merged export, exact frame mappings, publication observation and all four rehashed semantic controls remain unverified. Source planning inspection is not runtime qualification.

The earlier 180-second timeouts and compiled candidate remain in the [implementation checkpoint](PULSAR_NATIVE_MIXED_CLOCK_EXPORT.md). Current terminal commands, output, source records, fixture diff and process samples are retained in [this evidence packet](evidence/roadmap-2026-10-09/native-mixed-clock-runtime/checkpoint.json). Both runtime receipts remain separate. No reference training, real media, Stash server contact, device actuation, host-service changes, or human acceptance occurred.


## Successor — 2026-10-09

[RPC error correlation and mixed-clock runtime](PULSAR_HANDY_RPC_ERROR_CORRELATION.md) records the corrected error decoder and the first completed mixed-clock export regression, including all four negative/restoration controls. Historical results above retain their original scope. Runtime reliability, public integration and the complete roadmap remain open.
