# Pulsar items 2–3: resumed implementation

Status: **INCOMPLETE**. This is a successor to
[the handoff](PULSAR_ITEMS_2_3_HANDOFF_2026-09-28.md), within GB-01/GB-02 of
[the closure matrix](PULSAR_GOOD_PHASE_CLOSURE_MATRIX.md). It does not close
either item or the Good phase.

## Changes in this continuation

- CLI generation accepts `--inference-profile FILE`. The profile supplies the
  model input contract; the engine checks exact model/runtime identities,
  preprocessing, provider and resources. `--model-class-count` conflicts with
  a selected profile instead of providing a second input declaration.
- Desktop can load the same bounded JSON file. Both generation and preview
  requests carry the selected profile. Legacy requests without a profile retain
  the existing explicit CPU default. Unsupported provider pins reject.
- `pulsar model list` / `model status` query the authenticated engine's inference
  catalog. It reports the pinned runtime, CPU adapter limits, and an empty list
  of qualified profiles. `model profile` writes a custom declaration from model
  bytes and that engine runtime. It does not inspect detector quality or install
  a model. Desktop exposes the same support query.
- Profile hashes use the existing lowercase SHA-256 contract. Native workers
  recheck profile admission, and telemetry resets at every request boundary.
- Campaigns execute a streamed, hash-verified private runner snapshot. Their
  asynchronous supervisor bounds retained raw stdout/stderr, deadlines and
  cancellation, and kills the campaign-owned process group on exit. CLI SIGINT
  and SIGTERM request cancellation. Existing unrelated processes are untouched.
- The benchmark accepts a profile in either the legacy top-level field or
  `settings.inference_profile`; unequal simultaneous declarations reject.
- Invalid human-effort evidence leaves a failed, unscored result. Successfully
  bound evidence bytes are retained privately alongside their digest; the claim
  remains explicitly unauthenticated.
- A durable campaign start record and per-subject results support read-only
  inspection after interruption. Missing, truncated or mismatched results are
  unknown; the inspector never launches a retry or converts missing evidence
  into a passing result.

## Using the custom CPU path

The engine must already have been started with its intended pinned runtime
(`PULSAR_ORT_DYLIB`). Changing a client's environment does not replace the runtime
of an existing engine. Inspect current support first; do not terminate an
unrelated engine to change it.

```sh
pulsar model status
pulsar model profile /path/model.onnx --width 640 --height 640 --class-count 1 > /path/profile.json
pulsar generate /path/video.mp4 --model /path/model.onnx --inference-profile /path/profile.json --profile fast --output /path/output.funscript
node scripts/benchmark-campaign.mjs --inspect /path/pulsar-intake-RESULT
```

The enabled detector contract is RGB, NCHW, stretch, byte scaling by 1/255,
zero bias and YOLO channel-major output, at most 1024 pixels per input dimension
and 256 classes. The engine currently admits two CPU threads and a 1.5 GiB
worker memory budget. These are adapter/admission bounds, not measurements of
model requirements or quality. Unsupported providers remain unsupported even
when their Cargo feature is enabled.

## Validation

Final validation passed **425 Rust tests**, **64 JavaScript tests** and **112
architecture checks**. The Rust count includes the actual-worker package-origin
test, explicitly enabled after its default ignored run. Commands, source identity
and retained evidence are in [validation.json](evidence/items-2-3-2026-09-28/validation.json).
The archive contains the 12-scenario production campaign, raw artifacts/receipts,
final CLI evidence and test logs. Engine bootstrap/session credentials and large
runtime/executable snapshots are excluded.

The production campaign initially failed because this host's current FFmpeg
could not create an output thread within the historical fixture's 512 MiB
address-space limit. A direct FFmpeg reproduction failed identically outside
the campaign supervisor. Changing only that allowance to the production
engine's 1.5 GiB budget produced all expected frames. Changing OpenMP/OpenBLAS
thread environment variables did not fix the 512 MiB case. The fixture now uses
the production budget; production confinement/resource limits were not relaxed.

The first CLI fixture used flat pixels and correctly hit the existing refusal
to export unresolved motion gaps after generation. The final fixture uses
deterministic stationary texture and passes profile export, generation, motion
export and unsupported-provider rejection. The diagnostic failure log is retained
and distinguished from the final passing CLI run.

The authored ONNX graph is an input-dependent diagnostic graph with a synthetic
box, not a trained detector. Cgroup memory is the peak for the whole worker
process tree, not inference allocation. CPU session/run telemetry is not a
native per-node provider trace. Desktop controller/source compilation and CLI
execution do not establish native-window visual qualification.

## Acceptance reconciliation and remaining work

| Requirement | Current evidence / remaining gap |
| --- | --- |
| GB-01 production execution with immutable identities | Authored fixtures exercise production confinement and pinned worker/model/runtime inputs. Runner snapshot replacement is tested. Native dependency drift rejects. This does not prove every crash interleaving or all tool/shared-library immutability. |
| GB-01 retained outputs, errors and timing | Native execution receipts, raw program/lineage artifacts, bounded runner stdout/stderr and per-subject results are retained. Interrupted results remain unknown. Native decoder stderr is not yet retained separately from its typed worker failure. |
| GB-01 independent scoring, rights and splits | Independent annotation/oracle and intake rejection tests are exercised. Rights/provenance fields are structural claims; a permitted real-media corpus and independent references remain needed. |
| GB-01 actual human work | Optional evidence binding/retention is implemented. Five actual human-hours and authentic human review/annotation evidence are not supplied by synthetic records. |
| GB-02 profile selection and exact declarations | CLI, Desktop generation/preview, engine admission, worker admission and benchmark share the declared profile contract. Custom CPU support is explicit. |
| GB-02 model/runtime catalog | Runtime identity, adapter limits and custom profile export are exposed. No qualified trained-model catalog, licensed official model selection or accelerated provider implementation is delivered here. |
| GB-02 engine-authoritative qualified fallback | **Still unimplemented.** Existing protocol transition checks do not resolve qualification, schedule/persist fresh fallback attempts, or enforce fallback cancellation/revocation across those attempts. The current catalog advertises fallback unavailable and all selected backends remain strict. This is remaining software work, not merely missing benchmark data. |
| GB-02 quality/speed/memory per profile | Synthetic CPU execution and whole-worker memory evidence exist. Trained-model accuracy, reference-hardware throughput/cold/warm behavior, per-provider traces and accelerated/vendor coverage remain unqualified. |
| Interruption/recovery assurance | Campaign read-only inspection and bounded cancellation are exercised. Hard-kill/power-loss recovery, native cgroup orphan reconciliation and the complete fallback lifecycle campaign remain open. |

The earlier mandatory model-review requirement is superseded by the user's
current AGENTS policy. No model review was requested or run. Independent release
gates and human acceptance remain separate. No commit, push or physical device
actuation was performed; existing earlier work was preserved.
