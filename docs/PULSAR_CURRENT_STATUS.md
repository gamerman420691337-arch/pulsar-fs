# Pulsar current status

> GENERATED from [PULSAR_EXECUTION.json](PULSAR_EXECUTION.json). Edit the JSON; run `python3 scripts/execution_dashboard.py --write` from the repository root.

The [execution dashboard](PULSAR_MILESTONES.md) is the single current work queue and milestone ledger.

Reconciled **2026-10-30**. Roadmap **INCOMPLETE**; autonomous runner observation **active**.

**Working tree: UNVERIFIED.** Earlier timing failures and all qualification gates remain open; reduced-authority refusal is currently verified in-process, not through a separate CLI session.

**Next: CLIENT-CORRECTNESS — Close shared-client correctness and editing fault scenarios.** Exit required: Verify source/view/time correspondence and target loss/reacquisition through real consumers; exercise protected editing, review/commit/rebase/undo/restart and crash/storage faults in CLI/desktop. Retain real-media limitations under M1-DATA. CUDA unit-report dependencies and the reproduced HSP journal-contention readiness race have scoped repairs; complete remaining shared-client and editing fault scenarios without treating unit fixtures as qualification.

Last verified checkpoint: Engine **1926 passed / 117 ignored**, clients **703 passed / 15 ignored**, Protocol **406 passed**. Current reader41 compatibility patch: 6 parser tests, 7 reader tests (1 ignored), two actual App refusal cases, strict Engine library/test lint, formatting, architecture and 36 source guards pass. Broader suite and native consumer results remain separately pinned checkpoints. Original reader41 WAL mutation and the invalid zero-test cache run remain retained; all 25 acceptance gates stay open.

Native core/browser/restart/resource scenarios have scoped passing evidence. Real rendered acquisition, useful learned quality, installed-Stash compatibility, physical Handy synchronization and full platform/human/release acceptance remain open. The earlier startup failure remains unexplained.

Historical detail is retained in [PULSAR_STATUS_HISTORY_BEFORE_DASHBOARD.md](PULSAR_STATUS_HISTORY_BEFORE_DASHBOARD.md). Update `PULSAR_EXECUTION.json`, regenerate the views, and keep evidence scoped to its original subjects.
