# ADR B3-057: Preserve a live lease through native capture

Status: Private capture entry point and authored confined idle-expiry case verified. Public GPU live integration remains unfinished.

Add `capture_tracked_in_lease` alongside ordinary tracked capture. Carry the caller's absolute `Instant` through database-bound admission validation, preparation, native copying and retirement. Refuse an already expired or cancelled lease before opening the database or inspecting staging. The monitored operation uses the earlier of the inherited end and its existing local resource-budget deadline. Do not convert the remaining lease into a fresh full timeout. Ordinary callers retain the existing local-budget behavior and exact format-4/schema-20 admission checks.

Keep existing staged-process custody and its reaper/fence behavior on errors. Expiry does not permit freeing claims while a process or staging obligation may remain. A successful captured image is still returned only after exact worker/tree retirement and metadata verification; its storage/lease ownership remains with the image.

The native test spends one real second of a two-second lease before starting capture. An authored confined shell worker publishes a readiness marker in its owned staging root and idles without replying. An independent observer verifies that the child reached that phase while RAM/storage remained charged. Capture must return at the original lease end, within the declared local tolerance, and all child/group/staging/creation/accounting obligations must retire. The original tracked real-worker byte-capture/ownership case also passes. A normal unit check verifies expired/cancelled refusal before database or staging access.

This preserves cooperative deadline checks; it does not preempt arbitrary blocking filesystem or SQLite operations. Exhaustive host-I/O and crash qualification remains required. Public GPU live capture, cross-process monotonic deadline binding, observation/admission/stream integration, six-axis/latency, model/media quality, Handy, supported-platform and human release gates remain open. See [implementation evidence](../../docs/PULSAR_NATIVE_CAPTURE_LEASE.md).
