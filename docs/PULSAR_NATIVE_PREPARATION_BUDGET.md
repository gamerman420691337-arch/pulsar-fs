# Native preparation budget checkpoint, 2026-10-09

Status: PARTIAL. The adaptive budget is implemented and locally verified; actual native live readiness remains FAIL.

Native capture preparation now reserves the declared image size rounded to MiB, with a 512 MiB floor and 1 GiB ceiling. Both live CUDA and offline CUDA callers use the same helper. The existing aggregate ResourcePool must admit that reservation; focused tests demonstrate refusal without leaking storage or worker capacity. Output allowance remains declared size plus 16 MiB; CPU, lease, address-space and byte-verification behavior remain unchanged. This does not increase inference worker memory or bypass Engine admission.

Direct dependency-selected Engine suites pass 621 default and 623 CUDA tests, with 54 and 68 ignored respectively. Architecture and source guards pass 289 and 36. These direct Engine suites are not Cargo workspace gates. The offline locked CUDA release build passes in 3 minutes 21 seconds, with 25 existing warnings.

The source-matching release was tested using the authored scalar model and the full 1,945,994,000-byte native library selection. Actual cgroup memory.max is 1,073,741,824 bytes. Initial capture retires as expired at 30.18 seconds before Ready; reuse and successor inference assertions are not reached. The native test reports one failure and zero passes. The larger preparation budget has not established cold-start readiness. No real media, Stash service or physical Handy was exercised.

The final worker custody ledger has no groups, retired groups, staging or creation records. The owned Engine exits via SIGTERM. Source, patch, release identity, build/test logs, sampled observations and failure are retained under `evidence/roadmap-2026-10-09/native-preparation-budget/`. All 415 source records remain identical to the recorded checkpoint. Scratch build and run outputs are disposable after retention.

Cold native preparation and complete live readiness require further work. Native image reuse remains runtime-unverified. Real model quality, sterile holdouts, installed Stash, sustained latency, platform/offline matrices, physical Handy and human acceptance remain open.
