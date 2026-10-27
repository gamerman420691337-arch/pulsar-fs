# Portable action-origin timing proofs — 2026-10-09

Status: PARTIAL. A reproduced archival proof-verification defect is repaired in the current authored replay. Complete portable/mixed-clock publication and release qualification remain open.

## Reproduced defect and implementation

The predecessor (`7484b713892dc6f94d0dcfa41e5bea96217bbc58c12c7b08bfcabee7b8fc355d`) published a historical schema-3 timing proof. The authored fixture changed the first mapping's `original_source_frame` to 777, rehashed the bytes and updated both stored proof references. Publication observation still returned `verified`. The exact assertion is retained in `predecessor-archival-proof-diagnostic.json`: actual `verified`, expected `unverified`. This is a semantic defect, rather than a stale-hash test.

The shared effective-action walker now accepts retained archival links. It checks imported revision records against their retained declarations, resolves the explicitly selected historical producer through immutable namespaces, verifies its receipt/plan and canonical program, and composes the same production merge labels through edits, restoration and retiming. Carriers include the original manifest identity for archived producers; native schema-4 carriers omit it and preserve their wire shape. Historical jobs and paths never execute.

Placed archival publication uses that walker and retains schema 5 with exact action carrier selections. Observation rederives carriers, selections, mapping facts and counts from current retained evidence before calling the proof verified. Sampled legacy schema-3 mappings are also rederived: removing the ancestry section and downgrading cannot make an incorrect historical frame mapping verified. Unplaced legacy schema 3 retains its separate convention and permits no sampled mappings. Memory admission remains bounded and retained through mapping/export/proof construction.

The archived selected-producer guard is still enabled. Mixed producer/placement/sampling publication is not enabled by this checkpoint. Exact candidate rebase correspondence still refuses changed canonical motion without supported derivation; full multi-carrier portability remains work.

## Current evidence

The [packet](evidence/roadmap-2026-10-09/portable-action-proofs/checkpoint.json) retains source, commands, terminal failures, authored replay artifacts and an unqualified binary.

- Current default Engine suite: 639 passed, 55 ignored. Offline locked CUDA release and final source consistency passed. Architecture/source guards passed 294/36.
- Two complete CUDA suite runs each passed 640 with one failure and 69 ignored: one Stash restart lease error (`worker fence lease is still held`, `os error 11`), then one live-ingress `Option::unwrap()` failure at `live_captures.rs:234`. Both affected tests later passed in isolation. The first isolated restart attempt timed out at 45 seconds; a four-thread full suite timed out at 240 seconds. This is not a green complete CUDA gate.
- The unchanged 15-second readiness fixtures failed before source and generation publication. An extended test-only source diagnostic reached project creation but its CLI returned null; a later attempt failed readiness even at 60 seconds. They do not qualify the full forward/reverse/two-clone campaign or cold startup.
- A separate current-binary replay used the preserved authored predecessor clone, a fresh local mock Stash association and a new destination. It published schema 5, rejected six rehashed faults (mapping, namespace, receipt, selection, downgrade, and downgrade with ancestry removed plus a wrong mapping), and returned to verified after every restoration. The project master remained identical. It made two mock Stash queries and performed no generation or device activation. Readiness was 5.56 seconds; total replay 12.43 seconds. This verifies that replay path, not every clone or performance condition.
- The current observer verified an actual retained predecessor schema-3 proof in a separate reader probe.
- The predecessor reader correctly reports the actual schema-5 proof as unverified. It became ready in 36.27 seconds with the 60-second diagnostic allowance. The current reader became ready in 7.74 seconds after restart, but its observation CLI timed out at 30 seconds; schema-5 restart verification remains incomplete.

The source fixture adds twenty archived semantic fault controls to its existing native controls. Its whole-test budget is now 180 seconds to accommodate that work; its readiness remains 15 seconds and CLI timer 30 seconds. Diagnostic copies alter only readiness/whole-test allowances to 60/360 seconds. Those copies are explicitly functional diagnostics, not performance gates. No production deadlines change.

Current binary: SHA-256 `9b63c345fb11a4ac727d0f2e6085feea6ca26853864ad92922c23ba0506cea24`, 35,181,792 bytes. Source inventory: 422 records, six existing paths changed, 416 unchanged, no new source modules.

## Remaining scope and custody

Complete per-action correspondence across different historical producers and placements, native/archive mixtures, changed-motion rebases, CUDA historical receipt selection, collision/trim/ordering and old-reader compatibility. Re-run complete native campaigns and resolve cold startup and broad suite stability. Real media/models, nonlinear clocks, performance, Handy/six-axis physical behavior, Stash authentication/TLS/user-library behavior, platforms/vendors, offline product packs and human acceptance remain separate open roadmap gates.

Only authored fixtures are used here. Live references remain permitted for training; sterile references are held out for evaluation. Final reader/custody results and owned scratch cleanup are retained in the packet. No shared service/cache or unrelated edit is changed.
