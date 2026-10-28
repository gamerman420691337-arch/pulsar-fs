# Parallel implementation handoff: CPU-ROUNDTRIP

The CPU-BUILD and scoped CPU-ARCHIVE work is integrated in `54f9ab6`. The exact
committed source and test binaries agree with the retained receipts; the identity
list in the conversational handback was stale. A clean default App/Worker build
also passes. The execution dashboard retains the original failed compile and all
acceptance gates.

The next parallel assignment is **CPU-ROUNDTRIP**. Use a separate worktree based
on current `origin/main`; root owns browser timing/rollover and integration.

1. Execute actual confined CPU attempts with exact captured runtime declarations;
   package declaration tests alone do not establish execution or roundtrip closure.
2. Export, clone, join/restart, re-export and import a second clone. Check exact
   original and ancestor attempt/image/profile/model/dependency correspondence,
   inert history, and absence of revived execution, grants, leases or accepted evidence.
3. Exercise the preceding reader against the exact new subject and retain
   pre/post DB, WAL, SHM and fence identities proving refusal before mutation.
4. Verify bounded workers, staging, storage and memory retirement, including
   interruption/refusal outcomes. Keep live/fallback history refusals until those
   separate namespaces are complete. Make no device calls or qualification claims.
5. Return a focused commit and source-bound commands/receipts, update the existing
   JSON task, and leave final integration/push coordination to root.

The previous host closure now refuses changed `libstdc++` and `libgcc_s` bytes.
A separate explicit [current QA dependency declaration](/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-28/native-timing/current-qa-closure.json)
was used for the new confined browser regression. Pin the actual selected worker
and dependencies for the roundtrip; do not reuse old identity claims or infer
backend equivalence/qualification from this QA declaration.

The completed repair instructions below remain as technical context.

Suggested owner: the parallel implementation agent. Coordinate with the snapshot
publisher; use a separate worktree and branch based on the newly pushed main.
Other work may be in flight. Preserve edits by other agents and keep changes
within this task; the publisher owns the initial snapshot push and integration.

Start with [the execution dashboard](PULSAR_MILESTONES.md) and root `AGENTS.md`.
Those repair and focused archival tasks are complete within their recorded scope.
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
