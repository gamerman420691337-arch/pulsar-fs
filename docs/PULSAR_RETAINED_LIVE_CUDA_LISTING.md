# Browsing retained CUDA captures

`pulsar live-capture list-evidence PROJECT CURRENT_REVISION [--limit 1..16] [--after CAPTURE_EPOCH]` returns a bounded project-scoped index of retained capture identities. The default page size is 16. Entries are ordered by binary capture epoch, not capture time. `next_after` is the last returned epoch when another row exists; pass it as `--after` for the next page. A capture does not need to remain active.

The command requires current authentication, project Read permission and the current expected revision. Each page uses one Engine database transaction. An empty project or a cursor beyond the last entry returns an empty page. Queries do not change the project revision or create transfer leases. Page size is enforced in both CLI and Engine. Client hydration validates page bounds, project identity, ordering and continuation correspondence.

Listing reads bounded scalar identities rather than loading original evidence blobs or following recorded paths. It is a database index, not validation of the original execution evidence. Use `live-capture evidence` to inspect an entry, and `live-capture download-evidence` for original bytes. Both perform their existing independent checks. Across pages, concurrent retention changes may remove or add entries; this is keyset pagination rather than a persistent historical snapshot.

Verification passed in [the listing checkpoint](evidence/roadmap-2026-10-08/live-cuda-evidence-listing/checkpoint.json). Retention controls, desktop presentation and the remaining product, model/media, platform, Handy, human and release gates remain open.

The serial workspace gate passed 1,042 tests (55 ignored), and the serial CUDA Engine gate passed 555 (63 ignored). The shipped CPU-only CLI case passed with listing and all five downloads enabled, and all 268 structural checks passed. Independent readback confirms the discovered capture identity and exact original SQL/file bytes. All 366 source records match current files, snapshots and archives. The initial missing client adapter compile case is retained with its repair.
