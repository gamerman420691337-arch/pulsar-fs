Historical original-twenty execution campaigns
=============================================

The runner uses exact JSON.parse test-module values and full preserved production
source. It does not extract convenient functions, stub implementations, repair
legacy source, or convert current-contract tests into historical evidence.

Scratch copies are ignored. Each campaign retains original/execution manifests,
source and appended-test hashes, raw Cargo output, a per-test classification,
and the generated offline lock if available. No original lock was preserved.
ORT uses explicit load-dynamic rather than binary downloading; this and all
isolation changes are recorded. No inference/runtime/model execution is allowed.

Runner exit zero means all twenty assertions executed with preserved bytes
unchanged, not that Cargo or the legacy suite passed. Unavailable dependencies
or compiler failure remain explicit. Single-forward zero-cycle history is not
an accepted current independent-backward oracle. B1 remains incomplete without
its permitted corpus, statistical protocol and actual human pilot.
