# Pulsar consolidation register

Supporting integration record, 2026-11-06. [PULSAR_PROJECT.md](PULSAR_PROJECT.md) owns scope, API, delivery and agent coordination; [the execution ledger](PULSAR_EXECUTION.json) owns task/evidence state. This register is an inventory and disposition, not another work queue.

## Integration branch

`codex/pulsar-integration` in `/home/brianklam/Projects/Funscripts/pulsar-roadmap-m1` is the single integration authority. Its base was development snapshot `3e24247`, followed by canonical-document commit `26896ef`. The newer `consolidate/trunk-2026-11-04` source at `91e8c7e` supplies the composed architecture/assistant/device-control baseline. Dirty CPU Handy fixes are applied on top; their original files and patch remain retained. Main is not overwritten or force-pushed.

The snapshot and WIP production sources were identical. Git had recorded them as separate commits, causing add/add conflicts with newer consolidation work. Newer consolidated source was retained, the CPU patch applied with three-way context, and duplicate CLI tests introduced by the mechanical merge were removed in favor of the consolidated definitions. The Handy settings conflict retained declared firmware extensions and canonical range validation. No independent timing/device authority was introduced.

## Worktree disposition

Every registered worktree was inspected and its dirty files preserved before integration. External source worktrees and branches are retained in place: they were created by other owners and may still be in use. They are historical/handback inputs, not parallel trunks. No process was stopped and no external dirty file was reset or deleted.

| Worktree | Initial branch/head | Preserved changed/new files | Disposition |
| --- | --- | --- | --- |
| `/home/brianklam/Projects/Funscripts/Funscript-Hub` | `main` / `9ab7af7207c4` | 1 | Separate legacy main history retained; untracked spec byte-identical to preserved original specification. |
| `/home/brianklam/Projects/Funscripts/.pulsar-stash-research` | `detached` / `f51a5ad75c3b` | 3 | Three research docs byte-identical to integrated copies; no duplicate active plan created. |
| `/home/brianklam/Projects/Funscripts/pulsar-consolidate` | `consolidate/trunk-2026-11-04` / `91e8c7ef6a0f` | 0 | Clean source baseline integrated; original owner worktree retained. |
| `/home/brianklam/Projects/Funscripts/pulsar-cpu-build` | `cpu-roundtrip` / `8c026d306886` | 53 | Production Handy/token changes integrated; standalone bench/qualification scripts retained under legacy/bench-tools; old ledger differences preserved and reconciled. |
| `/home/brianklam/Projects/Funscripts/pulsar-roadmap-m1` | `codex/pulsar-roadmap-m1` / `3e24247f0718` | 38 | Current integration checkout; canonical docs and integration changes committed here. |
| `/mnt/4tb/tmp/funscripts-handy-play/src` | `agent/handy-bench-play` / `57df44127c8d` | 1 | Distinct attended-bench implementation retained under legacy/attended-bench and original history; direct device route is migration input. |
| `/mnt/4tb/tmp/pulsar-stash-explicit-multifile` | `agent/pulsar-capture-first-failure` / `dcd0222b345a` | 4 | Assistant fixture fix already represented; all unique ledger receipts retained in unified ledger. |

## Branch and source reconciliation

| Source | Disposition |
| --- | --- |
| `consolidate/trunk-2026-11-04` | Newer production source, Appendix E owner requirements, four additional gates and 268-record ledger imported. |
| `codex/pulsar-roadmap-m1`, WIP `8a4aee7` | Development source snapshot retained in ancestry; canonical document changes preserved. Original frozen gate/history files unchanged. |
| `cpu-build`, `cpu-roundtrip` | CPU archival/roundtrip source already represented; scoped receipts from the development snapshot restored without replacing newer evidence. Dirty source fixes integrated separately. |
| `agent/handy-bench-play` | Distinct direct-device bench client retained as source/reference and history; not wired as a second supported API. UNIFIED-API owns porting. |
| `origin/agent/handy-hsp` | HSP preload, speed cap, persistent connection/invert/actuator and Studio/Cinema work retained in legacy/handy-hsp and history; port through Engine admission, session limits and Stop. |
| Local `main` (`9ab7af7`) | Unrelated legacy open-fungen workspace/history, including NVIDIA GPU work, preserved without replacing the hardened workspace or changing license policy. |
| `agent/pulsar-macos-socket-probe` | Final transport/import/timing source byte-identical to consolidated source; standalone probe retained. Intermediate diagnostics preserved in ancestry; current full CI retained instead of replacing it with a manual-only job. |
| Other local/remote task branches | Already ancestors of consolidation; their implemented work and original histories retained. All initial branch refs receive local archive tags. |

For macOS parity, final `transport.rs` SHA-256 was `bf0f1d2086b12f6f65ad68b3e45216b9ecee60a4250ae50a6c179b21418e1c64` on the diagnostic and consolidated branches. Patch-equivalent and intermediate commits are not additional missing production fixes.

Existing legacy Buttplug, serial/TCode, VR, media/tracking/model/backend and GPU experiments remain discovery/porting inputs. Their original labels do not become supported-product claims. The canonical requirements and accepted architecture decisions define which behaviors must ship; useful implementations move behind the shared API rather than reviving the monolith.

## Preservation and evidence

Durable artifact root: `/mnt/4tb/artifacts/Funscripts/keep/pulsar-consolidation-2026-11-06/`. `inventory.json`, `worktrees-before.txt`, `refs-before.txt`, per-worktree `tracked.patch`, `working-files.tar` and `files.json` capture the original heads, dirty source, deletions, untracked files and SHA-256 identities. `archive-tags.json` records local recovery tags. No original refs were deleted or remote history rewritten.

All 281 predecessor evidence records are retained, plus failed and repaired integration-build receipts (283 records total). The default-feature all-target compile passes; the original E0063 failure remains preserved. Original outcomes, failures and pinned subjects are preserved; this is not acceptance of the new source. The initial integration history is committed at `c28b9d2` and published through [draft PR #34](https://github.com/gamerman420691337-arch/pulsar-fs/pull/34); the follow-up launcher repair is source-bound by its build receipt. All 35 initial branch tips are ancestors of the integration branch.

The canonical scope includes all original requirements plus the newer roleplay/consent/session/model-provenance/optional-image requirements. All 29 gates remain open. Build, lint, test and reconciliation logs are retained in the same artifact root.

## Initial ref register

Local archive tags preserve each exact initial tip. They are recovery anchors rather than active feature branches; publishing them is not needed for the integration PR.

| Original ref | Initial commit | Local archive tag |
| --- | --- | --- |
| `refs/heads/agent/handy-bench-play` | `57df44127c8d723dbba2abe6c0edbd6a2df31139` | `archive/pulsar-consolidation-2026-11-06/local/agent/handy-bench-play` |
| `refs/heads/agent/pulsar-absolute-connect` | `4f7b10c7ac3729b506e3cf473d7514acfc7627c2` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-absolute-connect` |
| `refs/heads/agent/pulsar-assistant-fixture` | `1744f47107a530cb561a901c089d307e92baa3d8` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-assistant-fixture` |
| `refs/heads/agent/pulsar-browser-fallback-archive` | `def3e1702ef94ff51cdeaeff9d0ec9f5ed1a8f4d` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-browser-fallback-archive` |
| `refs/heads/agent/pulsar-capture-first-failure` | `dcd0222b345af6da558bc36c0a0f00d6f94540fb` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-capture-first-failure` |
| `refs/heads/agent/pulsar-execution-evidence` | `358cf53d0794af6c7b489047fee27ea091242dad` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-execution-evidence` |
| `refs/heads/agent/pulsar-macos-socket-probe` | `5c5e1c35da346868193cb98b449fe1f238e8d8c8` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-macos-socket-probe` |
| `refs/heads/agent/pulsar-pr31-dashboard` | `b14de8ed0c5780ba19b250e5cd286c697deff698` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-pr31-dashboard` |
| `refs/heads/agent/pulsar-recording-boundary` | `5aa2626ab94d4c082b9e3ff10af72ef7b1efdda4` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-recording-boundary` |
| `refs/heads/agent/pulsar-reference-macos` | `9ac624492988b790c6fdbe1911ec96ca853e8c10` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-reference-macos` |
| `refs/heads/agent/pulsar-timing-exchange-diagnostics` | `75b7d9344f69f00821d5698430f85805e8f3646d` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-timing-exchange-diagnostics` |
| `refs/heads/agent/pulsar-timing-fixture` | `f432364eb45d5a3fee66d76eb740c89b411e3ff8` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-timing-fixture` |
| `refs/heads/agent/pulsar-timing-sweep-isolation` | `27e5a324d3e30b8ac858b06b9133ec09210bea25` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-timing-sweep-isolation` |
| `refs/heads/agent/pulsar-transfer-poll-diagnostics` | `9f81a96b4e21c63949f614e4acd55e94ffbcc926` | `archive/pulsar-consolidation-2026-11-06/local/agent/pulsar-transfer-poll-diagnostics` |
| `refs/heads/codex/pulsar-roadmap-m1` | `3e24247f0718d89eff74f8ac470d76177bda4d84` | `archive/pulsar-consolidation-2026-11-06/local/codex/pulsar-roadmap-m1` |
| `refs/heads/consolidate/trunk-2026-11-04` | `91e8c7ef6a0fa4ac110cec10709d13c050ecae71` | `archive/pulsar-consolidation-2026-11-06/local/consolidate/trunk-2026-11-04` |
| `refs/heads/cpu-build` | `3562757dbfd34b425c7a72127de66163dfcc1d07` | `archive/pulsar-consolidation-2026-11-06/local/cpu-build` |
| `refs/heads/cpu-roundtrip` | `8c026d306886030ed26d50ed96f553eaeea618b1` | `archive/pulsar-consolidation-2026-11-06/local/cpu-roundtrip` |
| `refs/heads/main` | `9ab7af7207c43550f55019dc7d1fba1f17ae55ee` | `archive/pulsar-consolidation-2026-11-06/local/main` |
| `refs/heads/wip/roadmap-m1-worktree-2026-11-04` | `8a4aee72c897f020f439608d376f1edf33035851` | `archive/pulsar-consolidation-2026-11-06/local/wip/roadmap-m1-worktree-2026-11-04` |
| `refs/remotes/origin/agent/handy-hsp` | `d888e4ffd7aab273283e57d913e87a02dd18d0c9` | `archive/pulsar-consolidation-2026-11-06/remote/agent/handy-hsp` |
| `refs/remotes/origin/agent/pulsar-absolute-connect` | `4f7b10c7ac3729b506e3cf473d7514acfc7627c2` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-absolute-connect` |
| `refs/remotes/origin/agent/pulsar-assistant-fixture` | `1744f47107a530cb561a901c089d307e92baa3d8` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-assistant-fixture` |
| `refs/remotes/origin/agent/pulsar-capture-first-failure` | `dcd0222b345af6da558bc36c0a0f00d6f94540fb` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-capture-first-failure` |
| `refs/remotes/origin/agent/pulsar-execution-evidence` | `358cf53d0794af6c7b489047fee27ea091242dad` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-execution-evidence` |
| `refs/remotes/origin/agent/pulsar-macos-socket-probe` | `5c5e1c35da346868193cb98b449fe1f238e8d8c8` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-macos-socket-probe` |
| `refs/remotes/origin/agent/pulsar-recording-boundary` | `5aa2626ab94d4c082b9e3ff10af72ef7b1efdda4` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-recording-boundary` |
| `refs/remotes/origin/agent/pulsar-reference-macos` | `9ac624492988b790c6fdbe1911ec96ca853e8c10` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-reference-macos` |
| `refs/remotes/origin/agent/pulsar-timing-exchange-diagnostics` | `75b7d9344f69f00821d5698430f85805e8f3646d` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-timing-exchange-diagnostics` |
| `refs/remotes/origin/agent/pulsar-timing-fixture` | `f432364eb45d5a3fee66d76eb740c89b411e3ff8` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-timing-fixture` |
| `refs/remotes/origin/agent/pulsar-timing-sweep-isolation` | `27e5a324d3e30b8ac858b06b9133ec09210bea25` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-timing-sweep-isolation` |
| `refs/remotes/origin/agent/pulsar-transfer-poll-diagnostics` | `9f81a96b4e21c63949f614e4acd55e94ffbcc926` | `archive/pulsar-consolidation-2026-11-06/remote/agent/pulsar-transfer-poll-diagnostics` |
| `refs/remotes/origin/codex/pulsar-roadmap-m1` | `f51a5ad75c3bb2dad1e2c22231b86bffa96c93c3` | `archive/pulsar-consolidation-2026-11-06/remote/codex/pulsar-roadmap-m1` |
| `refs/remotes/origin/cpu-roundtrip` | `8c026d306886030ed26d50ed96f553eaeea618b1` | `archive/pulsar-consolidation-2026-11-06/remote/cpu-roundtrip` |
| `refs/remotes/origin/main` | `06b2eb8f09abf8bb1ef85e2190582df29aeac954` | `archive/pulsar-consolidation-2026-11-06/remote/main` |
