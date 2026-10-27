# Captured CPU runtime execution

Status: implemented and exercised on Linux x86_64 with authored neutral fixtures.
This capability does not qualify a model, authorize automatic fallback or complete
the roadmap.

The Linux x86_64 Engine can select a private CPU dependency policy at startup with
`PULSAR_CPU_OWNER_POLICY=/absolute/cpu-owner-policy.json`. The policy describes
exact library bytes and loader aliases. It is configuration, not a credential;
keep it private to the current user. The Engine reads its bytes once at startup.
Changing the file does not change an already running Engine's selection.

## Selection and execution

The policy has `schema_version: 1` and a `native_dependencies` object with its own
`schema_version: 1` and `libraries` array. Each library supplies an absolute `path`,
SHA-256 `sha256`, `byte_len` and explicit loader alias `names`. The existing native
capture limits apply. A loader, libc and `libonnxruntime.so.1` are mandatory; the
owner must also supply every transitive dependency needed by the worker, runtime
and dynamic media tools. Listing required aliases does not prove completeness.

Model-backed generation and preview require an explicit CPU inference profile
whose runtime identity matches the policy. The policy runtime can differ from
the default runtime used by other configurations. Implicit CPU selection is
refused when this policy is configured. No policy preserves the ordinary CPU
execution path. CUDA keeps its existing separate owner policy and admission.

Before admission, a supervised capture hashes the selected files into a private
image. The worker starts through the captured loader with its cache disabled.
It receives captured libraries under the fixed guest directory
`/pulsar-cpu-libraries`; host library directories and the host loader cache are not
mounted. Explicit source, model and media-tool files remain separate read-only
inputs. Missing libraries must fail inside this namespace. The execution deadline
begins before mount/bootstrap setup and is retained through worker execution.
Capture has its own bounded preparation lease.

The worker checks the request, image, aliases, progress binding and original
deadline before inference, then checks the retained image again after execution.
Image storage and descriptor custody remain with whole-worker-tree retirement.
Progress uses the existing framed channel and the exact resume basis; a fresh
capture does not change its fixed runtime coordinates.

## Restart, resume and portability

Real job attempts retain a path-free CPU image declaration in their admission
transaction. Database reader version 36 prevents an older Engine from reopening
these attempts under ordinary host-library execution. Startup recovery leaves
captured attempts interrupted. An explicit resume requires the original policy,
image, runtime and checkpoint plus a fresh capture and resource admission.
Changing an ordinary interrupted job into captured mode requires a fresh job.
Preview remains transient and does not create fabricated job/attempt records.

Portable packages cannot yet carry this mode's complete attempt history. Export
and import refuse histories whose captured CPU declarations would be lost.

## Live capture

Explicit CPU live capture now uses the same owner policy on Linux x86_64. The
Engine declares the exact captured image, worker, model and profile before
startup. Capture, staging, mount/bootstrap, Ready, frame results and clean input
completion retain the original capture lease and mode. Ordinary host-worker
replies cannot substitute for captured replies. Start, continuation and prepared
browser successors keep the selected mode; cancellation and expiry retire it.

Recording pages retain the original manifest. Reader38 protects captured live
admission history; restarting preserves historical mode while leaving captures
retired. The exact preceding app refuses this history with DB, committed WAL,
SHM and worker-fence files unchanged in the native campaign. Portable transfer
of the complete captured/fallback history remains unfinished.

The native browser scenario verifies clean EOF and Engine-seeded successor
handover using decoded sterile frames and authored browser timing callbacks.
It does not establish rendered-browser acquisition or physical synchronization.
Retained timing metadata keeps its own memory budget until Engine destruction;
the test verifies exact worker-resource release first, then complete metadata
release within the same original cleanup allowance. See the
[live checkpoint](/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-24/captured-cpu-live/verification.json).

## Qualification boundary

A successful run proves execution only for its actual model, media, runtime and
host. It does not establish useful video-motion quality, equivalence to another
provider, resource performance, device synchronization or owner acceptance.
Environment inspection remains `report_only: true`, `qualified: false` and
`runtime_dependency_closure_verified: false`, including when it observes every
configured policy file. Actual closed execution and an inspection are different
evidence scopes.

Opt-in finite fallback now checks current accepted-equivalence records, expiry
and withdrawal before scheduling a fresh successor that preserves requested
semantics and retry limits. Its successful native campaign uses simulated
eligibility, so it supplies no measured equivalence or owner acceptance.
Browser/live equivalent-backend fallback remains open. A prior live startup
timeout also remains causally unresolved despite successful later runs.
Physical Handy, installed Stash, real-media,
platform, portable distribution and human/release gates remain open.

## Verified execution scope

The [native campaign](evidence/roadmap-2026-10-24/closed-cpu-runtime/native-cpu-006/campaign.json)
uses a real captured nine-library image and static media tools. An intentionally
invalid default runtime confirms explicit owner-runtime selection. Actual
Generate and detector Preview succeed; an interrupted job reuses checked progress
after Engine restart; cancellation publishes no candidate; committed motion stays
unchanged. Omitting the host-available `libstdc++.so.6` makes the captured runtime
fail at `dlopen`, with worker retirement retained and no candidate. ONNX Runtime's
loader error does not identify that transitive library by name.

An actual previous app refuses the new reader version with both existing DB and
committed WAL bytes unchanged. The campaign joins every owned process and removes
its private state. Earlier failures are retained, including the corrected duplicate
runtime identity and harness transform/diagnostic/coverage assumptions. These
results establish the tested execution and lifecycle behavior, not model quality
or general dependency compatibility. The [checkpoint](evidence/roadmap-2026-10-24/closed-cpu-runtime/verification.json)
records exact subjects, tests and remaining gates.
