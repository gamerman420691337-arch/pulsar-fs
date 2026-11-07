# Pulsar work routing

Start implementation, feature, planning and status work at
[the project specification and roadmap](docs/PULSAR_PROJECT.md). It owns feature
scope, delivery order and the supporting-document hierarchy. Its document catalog
covers technical references and preserved historical plans; do not revive their
old queues or append another dated status narrative.

Task state, dependencies and evidence belong only in `docs/PULSAR_EXECUTION.json`.
Follow its active slice and dependencies. Mark changed source unverified until a
fresh source-bound result exists; retain the last passing checkpoint and failures.
Scoped software results do not close unmet product/device/human acceptance gates.

Regenerate existing views with `python3 scripts/execution_dashboard.py --write`
and run `--check`. Use `--audit` on the evidence host when reconciling receipts.
Use relevant focused tests and real workflow checks; the roadmap defines release
checkpoint obligations. Update the existing app guide when user controls change.

## Integration and parallel work

`codex/pulsar-integration` is the integration branch. Read the canonical document's
API/ownership sections before dispatching agents or changing shared contracts.
The orchestrator owns specification, ledger and integration; assign agents exact
file/module scope on isolated short-lived worktrees. Hand back a commit, changed
paths, checks and limitations. Runtime authority stays in Engine through the
existing EngineApi/SessionClient seam. Preserve external dirty worktrees and use
the consolidation register for their disposition.
