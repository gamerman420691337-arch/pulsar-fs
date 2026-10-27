# CUDA archived producer identity — 2026-10-09

Status: PARTIAL. Historical CUDA producer receipt selection is repaired. This checkpoint does not qualify real GPU generation, placed CUDA source clocks, physical playback, or the complete roadmap.

A CUDA candidate retains the general producer receipt followed by its CUDA execution receipt and node trace. Archived inspection and the Stash lineage matcher previously assumed the last entry was the producer receipt. The existing authored CUDA portable lifecycle now exercises the actual scoped ArchivedGenerationStatus API and publication lineage matcher through import/restart and re-export/second-clone/restart. Before the production repair, inspection refused with `DependencyMismatch: archived generation correspondence changed`. The regression passes after repair.

Both consumers now resolve the producer using the existing typed CUDA role-ordering contract, with exact job and declared receipt identity checks. CPU candidates retain their existing convention. No arbitrary matching lineage entry is accepted. Both clones preserve identical historical origins and remain archival, inactive and unqualified. The authored fixture is unplaced and explicitly retains no sampled clocks. It performs no real GPU inference or device actuation.

Evidence: [checkpoint](evidence/roadmap-2026-10-09/cuda-clock-origin/checkpoint.json).

- Targeted regression: red exit 101 before repair; green after repair.
- Full CUDA Engine suite: 641 passed, 0 failed, 69 ignored.
- Full default Engine suite: 638 passed, 1 failed, 55 ignored. `worker_fence::tests::format3_creator_retires_exact_obligation_and_releases_claim` failed at worker_fence.rs:757 with `Custom { kind: TimedOut, error: "staging cleanup unconfirmed" }`. Its isolated retry's outer harness timed out at 65 seconds, without an inner terminal receipt. This gate remains unresolved; production cleanup deadlines were not relaxed.
- Offline locked CUDA release, final source consistency, runtime assembly and architecture/source guards (294/36) passed. Release emitted existing warnings.
- Source inventory: 422 records, three existing paths changed, 419 unchanged; no new source modules.

Runtime candidate: SHA-256 `9426b5fdd0ca2b37f3daae645f8f459262b478a9c79f4b6b70dd6d62b983c385`, 35182432 bytes; unqualified.

Remaining work includes cleanup and cold-start reliability, full native/archived/mixed-clock publication campaigns, placed CUDA clocks and real GPU execution, permitted live-reference training and sterile held-out evaluation, real-media quality/performance, platform packaging, and physical/human acceptance. Handy 2 Pro is the user-selected hardware; no credential environment variables are configured yet and firmware identification remains pending. Credentials must use the encrypted secret store; no plaintext secrets or physical-device commands were used here.
