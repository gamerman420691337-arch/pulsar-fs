# Exact merge action origins

Status: production merge kernel reports exact action inputs; Engine ancestry composition and durable clock proofs remain pending.

`merge_candidate_with_action_origins` uses the same implementation as `merge_candidate_preserving_protection`. The optional origin collector reports, for every output axis and action, its exact project timestamp, Current/Candidate input and original index in that input's axis. Existing production callers avoid allocating labels. There is no second write-mask implementation, value-based origin inference, new wire command or format change.

Protection and selection-boundary anchors remain Current even if the candidate has an equal-valued action at the same time. Sparse insertion and deletion retain the original input indices. Omitted axes remain Current, empty output tracks have empty labels, and deleted actions or gap envelopes create no fictitious action labels. This describes action ownership only; it does not certify interpolation or gap provenance.

Six new integration tests check exact and conservative boundaries, equal-valued actions with different origins, sparse insertions/deletions, protection, omitted/empty axes, gaps and malformed selections. A composed core check uses these labels with the existing selected-source-clock mapper: the 125 ms preserved boundary maps to source frame 1; the contributed 250 ms action maps to source frame 4 at 500 ms. A retained action at that same source time causes explicit collision refusal rather than deduplication or silent resampling.

The [source-bound checkpoint](evidence/roadmap-2026-10-09/exact-merge-action-origins/checkpoint.json) records 43 unit and 115 integration tests across every core integration target, 632 default Engine tests (55 ignored), 634 CUDA Engine tests (69 ignored), 292 static architecture checks, 36 source guards and a clean diff check. These are direct Rust source checks with existing dependency artifacts. A release executable was not rebuilt in this step; no native CLI, physical device or GPU vendor qualification is implied. All 420 source inventory records were rechecked, with only the core merge implementation and its integration tests changed from the prior checkpoint.

The Engine must still authenticate both input identities and compose these facts across commit, merge, edits, rebase aliases, undo/redo and source retiming. Historical contribution envelopes cannot substitute for this action-level correspondence. Durable versioned proofs, replay/restart and portable closure remain required before mixed-clock publication. Useful models and real-media quality, measured performance, platforms, Handy and human/release acceptance remain open.


## Native authored-edit clock origins — 2026-10-09

[Native edit origin verification](PULSAR_NATIVE_EDIT_CLOCK_ORIGINS.md) closes a reproduced CLI defect: a corrupted authored-edit receipt previously still permitted publication. The ancestry walk now verifies receipt/upload/CAS identities and replays values-only reconciliation, including exact authored/inherited intervals. Corrupt/missing evidence refuses and restoration permits valid publication. Source-bound release and native regressions pass; an earlier worker-fence lease failure and incomplete serial attempt are retained, with concurrency root cause unresolved. Per-action ancestry composition and durable mixed-clock proofs remain pending; roadmap status remains PARTIAL.
