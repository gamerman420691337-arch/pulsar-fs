# ADR B3-166: bounded retained local batch scheduler

Date: 2026-10-12. Status: implemented software contract; scoped verification. Full roadmap PARTIAL.

## Decision

Expose explicit `library-batch-run` and read-only `library-batch-workflow-status` over the existing Engine generation ledger and client neutral publication journal. Write a private reader-schema-2 workflow before any generation advance, retain immutable intake and acquired effect identities, and reconcile only monotonic actual Engine progress. Ready entries publish before later generation starts, preserving earlier success during later failure. Schema-1 history remains readable; no Engine schema or script-format change.

Use read-only job observation while queued/running/assistant-paused; only completed jobs advance their existing generation chain. Failed, cancelled, interrupted or unrecognized states request attention with no replacement job. Reuse an unresolved Engine outer request identity exactly. Share the existing publication helper, exact stage/request/filesystem identity and acknowledgement fences rather than another exporter or relaxed recovery rules.

Bound explicit scheduler decisions and polling delay. Distinguish attempts, polls and budget exhaustion from completion. Status reads current generation/job state and retained publication history without writing journal reconciliation. Keep accepted publication history distinct from Engine truth, fresh file observation and qualification.

## Evidence and remaining scope

The [checkpoint](../../docs/evidence/roadmap-2026-10-12/library-scheduler/checkpoint.json) retains suite/strict lint/platform source/build/format/architecture results, exact source/binary identity and actual network-disabled native one-step/restart/per-entry publication/completion/repeat/late-failure checks. Compare [scheduler contract](../../docs/PULSAR_LOCAL_BATCH_SCHEDULER.md). Continuous watching/intake, partial cancellation/disposition, migration/GC, unknown local acknowledgement resolution, broader fault/platform/model/media/device/human and release qualification remain open.

Configured verification passes 1,516 selected-package tests and all eight retained checks. Native one-step/restart/per-entry completion has ten actual generation advances and 118 read-only polls, with eleven durable batch replies including intake. A later invalid WAV preserves the first publication and original failed job across restart/repeat. Previous publication and controller regressions pass. Nine Engine instances are joined; owned scratch is audited and removed. These scoped results do not close remaining roadmap qualification/disposition gates.
