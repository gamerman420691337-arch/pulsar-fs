# Request-bound CUDA model adapters

The worker's detector and temporal script-position adapters now share a model
session layer. CPU execution retains its existing preprocessing, decoders and
telemetry. Linux builds with the `cuda` feature can construct a checked CUDA
session from a validated worker request. Production CUDA admission remains
disabled; this is an implemented adapter prerequisite, not a GPU job launch.

The request factory derives model, profile, runtime and device declaration from
one request. It checks supported preprocessing and thread limits, verifies the
model and every declared captured-library alias against bounded immutable-byte
identities, and selects the runtime/driver from that declared image. Successful
finalization binds the receipt to request/profile hashes, attempt/job IDs and
model/runtime/owner-policy identities. No provider substitution is permitted.

CUDA preprocessing, native execution and output decoding share one failure
scope. Any failure prevents subsequent calls and successful receipt creation.
Finalization consumes the session and requires the decoded successful-run count
to agree with the original CUDA node trace. Generation and captured-pair paths
retain this receipt. [Original CUDA evidence egress](PULSAR_CUDA_RESULT_EGRESS.md)
now transports the dedicated receipt and raw trace for parent consistency
checks. CUDA preview remains disabled pending its complete launch/result path.
CPU preview remains supported.

## Verification scope

The [validation packet](evidence/roadmap-2026-10-01/cuda-model-adapters/validation.json)
records the exact source and compiled subjects. The authored namespace probe
calls the actual `from_request` factory with an authored private policy and the
declared captured image. It does not obtain production engine admission.

The final default workspace/all-targets suite has 885 passes, zero failures and
seven ignored qualification cases (55 suite summaries, including 17 empty
example suites). The CUDA-feature engine suite has 426 passes, zero failures and
one ignored case. All 96 configured JavaScript cases pass without skips; 219
structural checks, feature/default builds and the diff check pass. The packet
retains 169 production and 117 verification paths, matching source archives,
31 unchanged executable/spec identities, all 74 native-library source readbacks
and 121 retained artifact hashes. The failed cleanup rename is retained as a
historical compile log without an archived failed source subject.

Four real RGB inputs exercise each adapter: temporal positions are approximately
0.25, 0.375, 0.5 and 0.75; detector scores are approximately 0.6, 0.625, 0.65 and
0.7 with checked class/box geometry. Float32 normalization/reduction is compared
within 1e-5, and raw values are retained. Each positive path has four successful
decoded runs and twelve CUDA computation events. Six refusal cases cover an
unavailable ordinal, CPU-only graph, malformed RGB input, invalid temporal
output, invalid detector output and a preinitialized runtime. Failed input or
decoding additionally refuses reuse and receipt creation. Terminal capture
cleanup is independently read back.

## Remaining integration

The caller still owns trusted image/device mounts, aggregate reservations,
process supervision and private output custody. Authored policy metadata and a
bound request hash do not establish those guarantees. The research namespace
does not add a physical RAM cgroup or whole-device VRAM enforcement.

Durable GPU admission and an older-writer compatibility barrier, confined
production launch, cancellation/restart fencing and durable accepted-evidence
membership remain required. Original raw-trace egress and parent consistency
verification are implemented separately; they do not establish launch authority.
No production GPU jobs, private
media training/evaluation, model accuracy/performance, physical Handy movement,
platform release or human acceptance are qualified by this increment.
