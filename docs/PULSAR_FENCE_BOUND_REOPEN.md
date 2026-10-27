# Bound worker-fence reopen, 2026-10-09

Status: the specific immediate bound-reopen test race is reproduced and corrected. Roadmap PARTIAL; no general claim that all lease races are eliminated.

The [checkpoint](evidence/roadmap-2026-10-09/fence-bound-reopen/checkpoint.json) binds 408 source records, one test-only changed path and 407 unchanged. All edits are inside `worker_fence.rs`'s `cfg(test)` module; its production prefix is byte-identical. Production still refuses a held lease. No unlocking, admission retry, recovery policy, timeout or protocol behavior was changed.

The previous CUDA suite failed `format3_missing_roster_and_unrecorded_roots_refuse_without_rewrite` at its final immediate bound reopen with "worker fence lease is still held". A controlled serial fork drives that same format-3 bound-open path: the child inherits the CLOEXEC lease descriptor, the parent drops its owner, and immediate reopen fails. The diagnostic variant fails in the initial run and three of three repeats. Source and logs are retained under `diagnostic-red/`; this deliberately failing variant is not product code.

The corrected controlled gate confirms the exact held-lease/`WouldBlock` error before exec. After allowing the child to exec, the exact bound reopen succeeds while the child is still alive. The test checks kernel child status and kills/reaps only its owned child. This distinguishes inherited descriptor lifetime from a parent-side retained owner or invalid ledger. The existing native gate remains ignored in ordinary suites because raw fork inherits other test threads' descriptors; it was explicitly run serially here.

The test harness extends its existing bounded fork-lease wait to a supplied bound-open operation. It retries only the exact held-lease message paired with `WouldBlock`, for at most the existing three seconds. Other errors return immediately. Negative cases now assert the actual missing-roster and unrecorded-resource errors, not merely any failure, and still verify unchanged ledger bytes and canary contents. Successful cleanup still requires the original ownership scope and minimum version three. A legitimate busy production lease remains refused.

Fresh checks pass 1,135 workspace tests (63 ignored), 622 CUDA Engine tests (68 ignored), 289 architecture checks and 36 source guards. The CUDA count includes 610 library tests plus 12 integration tests; the preceding failed invocation stopped after 609 library passes and one failure. Three additional full parallel CUDA suites and three serial controlled-fork repeats pass. The targeted negative-validation test also passes. These are current-source results, not a retry-to-green on unchanged failing source.

The release executable remains byte-identical to the preceding alias-proof checkpoint, and its native RGB/Conv evidence is inherited rather than rerun. Workspace tests rebuilt the default development executable; it is not the preceding CUDA development supplier and must not be used as one. The previous red checkpoint and its artifacts remain intact.

Diagnosis used the repository's existing lease contract and the diagnosing-bugs workflow: controlled red reproduction, hypotheses distinguished by exec/child liveness, test-only correction, and original parallel-suite stress verification. No temporary debug instrumentation remains in product source.

Readiness remains roughly nine seconds in the preceding release measurements. Continuous acquisition/handoff, long-duration capture-to-command, live six-axis, useful model quality/real annotations, remaining desktop combinations, mixed clocks, installed Stash, platform/offline, physical Handy and human/release acceptance remain open. No media, training, user service contact or physical playback occurred in this test-only campaign.
