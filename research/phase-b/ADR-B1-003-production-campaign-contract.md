# ADR B1-003: production campaign execution and neutral-output scoring

Status: implemented development path; items 2 and 3 remain open.

## Decision

`scripts/benchmark-campaign.mjs` stages rights-checked source/reference inputs,
invokes an explicitly identified local `benchmark-generate` executable, and
scores its independently converted neutral output. It does not use supplied
candidate annotations as generated output. Campaigns run sequentially.

The engine harness calls the production confined launcher. It copies and checks
the worker executable before execution, preserves the complete worker manifest,
records elapsed time and typed worker results, and refuses existing receipt
paths. This is an offline qualification harness, not a new authorized command
on the shared project engine. It does not commit projects or actuate devices.

Neutral output uses checked Rust deserialization. Segmentation splits at pauses,
direction reversals, and missing intervals. It preserves integer nanoseconds,
including values above JavaScript's exact integer range. Positions round to one
part per million. A separate standard-funscript adapter reports its coarser
millisecond and percent quantization. Neither adapter sees reference strokes.

Human effort is optional supplied evidence, bound to subject, source digest,
candidate digest, actor and session. An attached evidence blob must match its
digest. Structural checks do not authenticate the human or establish that their
measurements are truthful. Worker elapsed time is never converted into human
editing time. Absent human evidence remains absent.

## Inference profiles

Model-backed campaigns require an explicit profile matching model/runtime
identities, input tensor contract, preprocessing and thread budget. Current
campaign admission supports the implemented CPU RGB/NCHW stretch adapter only;
unsupported providers and preprocessing are rejected, not substituted. Detector
session construction explicitly registers CPU with registration failure treated
as an error.

A declared profile is not an observed provider trace. Worker receipts now report
successful explicitly configured CPU session runs and inference elapsed time.
They explicitly distinguish this from native per-node provider tracing. Campaign
execution receipts record measured whole-worker cgroup peak memory, not a claim
about inference-only allocation. The shared launcher uses the same admission
checks, and shared-engine generation binds the resolved profile digest into
attempt dependencies. New native providers, equivalent fallback execution,
user-selectable profile surfaces, and broader per-profile qualification remain open.

## Commands

```sh
cargo build -p pulsar-engine --example benchmark-generate --offline
PULSAR_BENCHMARK_RUNNER="$PWD/target/debug/examples/benchmark-generate" \
PULSAR_BENCHMARK_WORKER="$PWD/target/debug/pulsar" \
node --test scripts/benchmark-campaign.test.mjs
node scripts/benchmark-campaign.mjs /absolute/path/to/campaign-config.json
```

The worker binary is separately built and identified. A rebuilt harness does not
imply a rebuilt worker. Missing Linux confinement support is a failure, not a
reason to execute an unconfined substitute.

## Remaining acceptance work

- Trained-model/real-media runtime qualification beyond the authored ONNX fixture.
- Broader engine/client profile integration and explicit fallback lifecycle.
- Model/runtime catalog and supported resource/profile coverage.
- Timeout, cancellation, corruption and interruption campaigns.
- Broader review semantics beyond the implemented global/stroke interval mapping.
- Real-media corpus/reference evidence and actual human-effort records.
- Final requirement-by-requirement assurance and external review.

Synthetic process fixtures establish wiring only. Successful stationary-media
execution does not establish tracking quality, realtime performance, memory
qualification, physical safety, or whole-item completion.
