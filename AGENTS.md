# Execution tracking

For implementation, status, or roadmap work, start with
[the execution dashboard](docs/PULSAR_MILESTONES.md). Its queue and dependencies
drive current work; its linked specification and decisions retain authority.

Update `docs/PULSAR_EXECUTION.json` as work changes. Mark a changed working tree
unverified until a fresh source-bound result exists, and retain the last passing
checkpoint separately. Update existing tasks rather than appending dated status
narratives. Scoped software results leave unmet parent acceptance criteria open.

Regenerate the views with `python3 scripts/execution_dashboard.py --write`, then
run `--check`. Run `--audit` on the evidence host when recording or reconciling
retained receipts. The dashboard's operating rules define evidence and closure.
