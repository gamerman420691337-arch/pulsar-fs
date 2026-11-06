# Parallel implementation handoff: CPU-BUILD

Suggested owner: the parallel implementation agent. Coordinate with the snapshot
publisher; use a separate worktree and branch based on the newly pushed main.
Other work may be in flight. Preserve edits by other agents and keep changes
within this task; the publisher owns the initial snapshot push and integration.

Start with [the execution dashboard](PULSAR_MILESTONES.md) and root `AGENTS.md`.
The first task is `CPU-BUILD`, followed by focused `CPU-ARCHIVE` verification.
The full M1–M8 roadmap and independent acceptance gates remain open.

## Repair scope

Primary ownership: `crates/pulsar-engine/src/project_packages.rs` and focused
portable-CPU tests. Change another production path only where a reproduced
compile or correspondence failure requires it, and report that expansion.

The retained first compile has four Engine errors:

- `check_cpu_origin_objects` refers to `crate::project_package_imports`; that
  module is nested under `authority`, alongside `project_packages`.
- CPU declaration collection in the later assembly loop refers to `tx` and
  `budget`, which belong to the earlier database-snapshot block.

Collect each attempt's checked image declaration using
`cpu_runtime_db::retained` inside the same transaction that captures the attempt
manifest. Account for metadata within that snapshot's budget, then move the
checked declarations through the returned snapshot into later assembly. Bind
them to exact original attempts, refuse missing/extra correspondence, and retain
the no-filesystem-I/O-under-authority-lock discipline. Reopening mutable history
after the snapshot would lose that correspondence.

## Preserve the contract

- Keep format 15 conditional; legacy packages retain their original form.
- Preserve successful, failed, cancelled, interrupted and captured-running
  attempts as inert history through current and ancestor namespaces.
- Keep exact image/profile/model/dependency checks and bounded graph/metadata
  admission. Importing history installs no policy and recreates no execution,
  grants, leases or accepted evidence.
- Retain fallback/live-history refusals until their complete namespaces exist.
- Keep earlier failures and qualification limitations. Synthetic/native test
  success supplies no model-quality or physical-device acceptance.

## Verification and handback

Mark the working-tree build unverified in `PULSAR_EXECUTION.json` before code
changes. Begin with a bounded Protocol/Engine/client library test build:

```sh
timeout 360s cargo test -p pulsar-protocol -p pulsar-engine -p pulsar-clients --lib --no-run
```

Use the project's existing SSD build cache and current environment conventions;
do not create a build tree on the HDD. Run the focused format-15, CPU declaration,
stripping/substitution/ancestor and unchanged fallback-refusal tests. Reuse the
owned private-RAM supervisor for the full Engine suite when needed. Keep all
processes joined and preserve exact source/binary identities and failures.

Return the commit or patch, affected paths, exact commands/results and remaining
failures. Update the JSON ledger and regenerate/check its views. A passing
compile does not close `CPU-ROUNDTRIP`: the actual export/clone/restart/re-export/
second-clone and preceding-reader campaign still needs independent execution.
Leave integration/push coordination to the publisher; make no device calls.
