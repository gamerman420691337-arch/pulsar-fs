# Closed CUDA runtime checkpoint

Date: 2026-10-09
Status: PARTIAL; decoder compatibility and production CUDA workflow remain open.

CUDA inference and memory-observation confinement no longer bind broad host OS library roots or the host linker cache. Captured, held library aliases populate anonymous read-only default library roots. Read/hash inspection roles retain ordinary OS access. CUDA tool paths are explicitly bound. Existing byte proofs, resource ceilings and deadlines remain unchanged.

Direct cached-dependency Engine suites pass 622 default tests (55 ignored) and 624 CUDA tests (69 ignored). An explicit native regression passes in both configurations: actual bubblewrap roots contain the held alias bytes and identity, exclude undeclared host libraries, and refuse writes with EROFS. These direct builds are not a Cargo workspace gate or a full SDK workflow. Architecture checks pass 289; source guards pass 36; diff whitespace checks pass. Offline locked CUDA release build passes in 228.88 seconds.

The source snapshot records 415 paths: three Engine paths changed and 412 unchanged relative to the preceding cuDNN diagnosis. The inherited client placement-seek repair is included in the retained release.

Installed FFmpeg and FFprobe dependency inventories each contain approximately 164 entries, exceeding the native image ceiling of 128 libraries if included indiscriminately. Inventory lines are not an independently deduplicated library count. Closing ambient fallback therefore leaves CUDA file-decoder compatibility unresolved. A bounded decoder closure or separate decoded-frame boundary must be verified before full generation is claimed. The matching cuDNN policy remains a diagnostic candidate, not a promoted qualification.

The private desktop session ended at its 180-second timeout (exit 124). Window lookup occurred after the display was unavailable and exited 139; no repaired-window or generation assertion was reached. This failed observation is retained. Final worker custody is empty and no process executable or working directory references the owned scratch.

Evidence is in [closed-cuda-runtime](evidence/roadmap-2026-10-09/closed-cuda-runtime/), including source snapshot, patch, exact build/test results, static checker, dependency inventories and release identity. Test binaries and copied build cache are disposable. Real media/model quality, full Stash, live latency, portable release, physical Handy and human acceptance gates remain open.
