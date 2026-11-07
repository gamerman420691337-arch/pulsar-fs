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
