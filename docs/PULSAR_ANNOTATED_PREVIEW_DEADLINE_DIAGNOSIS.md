# Annotated preview capture deadline diagnosis

Date: 2026-10-09
Status: Reproduced; production repair and roadmap qualification remain PARTIAL.

## Exact public failure and passing controls

The 417 production source records and all seven Linux runtime-bundle files match the preceding desktop completion checkpoint. The original authored four-second stereo clip, Conv model, target view, strict CUDA profile and explicitly selected 81-library diagnostic policy are reused. No owner configuration, shared service, supplier bytes, private media, Stash installation or physical device changes.

Two public source-time previews succeed in 34.510 and 30.758 seconds. Three public placed previews succeed in 14.990, 14.655 and 14.312 seconds; the fourth fails in 30.010 seconds with `Unavailable: native capture absolute deadline exceeded`. Two overlapping placed requests both fail with that exact error, in 30.025 and 30.036 seconds. These observations establish intermittence and a failing concurrent cohort, not a latency distribution or a sole cause.

The placement matches the original desktop trim, anchors and rate. Successful placed previews select source frame 8 with predecessor 7 at project 1.25 seconds. Independent byte comparison verifies both target frames against the retained native-decoded RGB sequence. Each success reports one CUDA inference and twelve CUDA computation nodes with no CPU computation nodes; position is 0.49511462450027466 and remains explicitly unqualified. The source-time control selects frames 8/9 instead. Generation's neutral first anchor is not substituted for the predecessor-aware preview result.

## Captured pressure and hypotheses

The failing sequential capture has written about 826 MB of the declared 2,434,823,256 bytes near its deadline. Its 1 GiB memory allowance is almost full, dominated by file cache. The last sampled worker has 1,895 memory max events, zero OOM events and zero CPU quota throttles. Twenty-eight of 59 worker observations are dirty-page waits; other observations include page, journal and block waits. The concurrent cohort similarly reaches the 1 GiB limit and has many dirty-page waits. Each concurrent sampler observes both children; its counts must not be summed as independent worker samples.

The displayed predictions were: retaining clean captured pages may impede later writes; supplier-read cost may explain the delay; shared I/O contention may explain the timing variation. The failed copy has not finished, so post-copy verification cannot explain that particular deadline. No global dirty limits, cache eviction, scheduler, WBT, driver, supplier or shared service settings are changed.

## Negative cache-advice experiment

A separate C++ copy diagnostic uses the full declaration under a systemd-owned 1 GiB memory limit, one-core quota and 1536 MiB address-space limit. It attempts copy-on-write first, otherwise performs bounded 8 KiB copies, checks original metadata, fsyncs each output, verifies its expected SHA-256, makes it read-only and syncs the directory. A final full hash pass is required for success. The experimental variant additionally requests `POSIX_FADV_DONTNEED` only on completed, synced, verified owned destination files. It never advises or evicts supplier files.

Both baseline and advised runs expire at the 75-second diagnostic deadline, after completing 61 libraries totaling 726,613,072 bytes. Baseline reaches that point at 60.127 seconds; advice reaches it at 73.965 seconds. The baseline service records a 1 GiB memory peak; advice records about 679 MiB. Host cache/load and run order are uncontrolled. This is neither an exact Rust capture implementation nor native preview qualification, and neither run completes all hash proofs. The experiment does not support adopting per-library cache advice as the repair. Addressing pressure within the large-file copy is the next candidate to test through the public preview loop.

## Harness negatives, retirement and retained evidence

The first placement harness reads the wrong response field and receives `source placement is not active`; it is corrected before the measured placed campaign. Its four refusals are retained separately. The measured failing campaign also checks cleanup too early, then terminates its Engine. The worker is subsequently confirmed absent; a new owned Engine reconciles the retained custody. The first recovery launch omits the pinned runtime and refuses `CUDA owner policy differs from the engine's pinned runtime`; the corrected launch passes readiness and reconciliation. Both recovered states have empty groups, retired custody, staging and creation records. These are harness limitations and recovery observations, not a claim that the failed request immediately cleared custody.

The first standalone service launch lacks user-bus environment and is refused before copying. The corrected launch uses the existing user bus. The diagnostic compiler succeeds with a misleading-indentation warning; that warning is retained in the compile record. All live handles are terminal before owned scratch deletion. State databases, session credentials and owner tokens are excluded from retention.

[Evidence packet](evidence/roadmap-2026-10-09/annotated-preview-deadline/) retains the exact harnesses, public command results, sampler records, successful preview pixels/receipts/traces, recovery fences, copy probe source and service logs, source snapshot and artifact manifest. The harnesses are historical run artifacts with absolute owned paths; reproduce them in a fresh private scratch directory, not inside the retained packet. Production source remains unchanged. Architecture/source guard results are retained; unchanged Rust workspace suites are not repeated.

Native preview/startup reliability, live latency/authority/reuse, useful held-out model quality, installed Stash, broader fault/retention and platform/offline qualification, physical Handy and human/release acceptance remain open. The full roadmap objective is unchanged.
