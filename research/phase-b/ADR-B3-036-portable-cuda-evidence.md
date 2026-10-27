# ADR B3-036: preserve accepted CUDA evidence as inert portable facts

Status: software integrated; isolated consistency and preceding-reader checks pass; main-checkout Rust/native/feature/build gates pass; application qualification remains PARTIAL after retained failures.

CUDA completion appends its dedicated execution receipt and node trace after the general worker receipt. Treating the last lineage member as the general receipt selects the trace, while omitting accepted membership loses the correspondence between candidate, attempt and original request. Portable format 10 therefore records explicit accepted-candidate membership, canonical request digest and distinct receipt/trace identities and roles. CPU origins omit the new optional field and retain their existing format requirements.

Capture checks the existing accepted membership against completed job/attempt/candidate scope and original bytes. Import validates original request, receipt and trace correspondence without resolving original paths or granting device or execution authority. Clone retains original namespaces and clears active job origins; nested original format declarations propagate the format-10 requirement through re-export.

Schema 18 is a conditional archival reader barrier. Its exact single-row marker records whether worker-fence, CUDA-intent and accepted-evidence tables were required at publication. Archival-only clone does not create those active tables. Later genuine activation adds requirements transactionally; missing required tables cannot be forgotten. Placement initialization preserves newer barriers. Read-only startup verifies retained CUDA anchors before writable SQLite initialization. Existing table owners still enforce their individual layouts and contents.

Ready export and clone publication arm the barrier in their existing transactions. A preserved schema-17 application reader opens a consistent schema-16 positive control, then refuses exported and cloned schema-18 subjects while preserving committed database/WAL bytes. Authored CUDA traces prove consistency only. Parallel engine testing encountered a retained lease-contention failure; unchanged serial testing passed, but the cause is unproven. Read-only SQLite may create an empty WAL when none exists, so sidecar noncreation is not claimed.

Production GPU admission, native-image/device lifetime qualification, actual trace authenticity, model/media quality, physical Handy operation and release acceptance remain separate obligations. See the [implementation status](../../docs/PULSAR_PORTABLE_CUDA_EVIDENCE.md).
