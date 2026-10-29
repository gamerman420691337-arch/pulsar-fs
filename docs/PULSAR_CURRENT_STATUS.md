# Pulsar current status

> GENERATED from [PULSAR_EXECUTION.json](PULSAR_EXECUTION.json). Edit the JSON; run `python3 scripts/execution_dashboard.py --write` from the repository root.

The [execution dashboard](PULSAR_MILESTONES.md) is the single current work queue and milestone ledger.

Reconciled **2026-10-29**. Roadmap **INCOMPLETE**; autonomous runner observation **active**.

**Working tree: VERIFIED.** Pair-gate starvation is reproduced and repaired; the matching 120s/10s-lease stream now reaches EOF and exact reviewed export. Earlier generic timing_exchange_unconfirmed cleanup cause remains unresolved. Freshness-margin warnings, the Engine harness deadline and full runtime/model/platform/physical/human acceptance remain open.

**Next: BROWSER-DEMO — Verify the real rendered-browser-to-recording/export demo.** Exit required: Localize the retained generic timing_exchange_unconfirmed cleanup failure using available first_failure diagnostics, then establish repeatable reliability without extending original deadlines. Pair-gate starvation has a deterministic repair and a matching 120s/10s-lease rendered EOF-to-export checkpoint across 23 records, but does not explain that generic IPC failure. Finish applicable pending replacement, interruption and unknown-outcome coverage; verify useful fresh visible output at the admitted lead, retain harness-limit counterevidence and exact source/clock/CPU correspondence. Selected software checkpoints do not qualify real media, models or physical devices.

Last verified checkpoint: Engine **1890 passed / 110 ignored**, clients **700 passed / 14 ignored**, Protocol **397 passed**. Engine: pinned causal HSP repair, full default-thread suite. Clients: current pair-gate repair, full suite with one test thread plus 161 browser JS tests and selected 120s rendered workflow. Protocol: separate pinned checkpoint. Generic timing cleanup, freshness/usability, harness shutdown and physical qualification remain unresolved.

Native core/browser/restart/resource scenarios have scoped passing evidence. Real rendered acquisition, useful learned quality, installed-Stash compatibility, physical Handy synchronization and full platform/human/release acceptance remain open. The earlier startup failure remains unexplained.

Historical detail is retained in [PULSAR_STATUS_HISTORY_BEFORE_DASHBOARD.md](PULSAR_STATUS_HISTORY_BEFORE_DASHBOARD.md). Update `PULSAR_EXECUTION.json`, regenerate the views, and keep evidence scoped to its original subjects.
