# Authenticated automatic live-pair inference

Capture leases can now admit an explicit temporal model/profile and automatically consume their latest captured RGB pair in one persistent confined CPU worker. The shipped CLI crosses the actual control endpoint, bulk pixel endpoint, engine scheduler and native ONNX worker. Results are current ephemeral inferred estimates, not committed motion or device commands. Native acquisition, qualified model quality, GPU six-axis behavior and physical dispatch remain open.

The capture options file optionally adds an `inference` object containing an absolute bounded `model_path` and an explicit declared profile. Omit it for a pixel-only lease. Read, ImportSource and Generate grants, paired ownership, exact project revision, current boot and validated clock/lifetime remain required. The engine binds the profile to its pinned runtime and worker. Unsupported providers/preprocessing, excessive budgets and mismatched runtime admission refuse. There is no automatic model or fallback selection.

Generate the complete fixed temporal profile and insert that object into the existing capture options file:

```sh
pulsar model profile /absolute/path/to/model.onnx --width 64 --height 64 --class-count 1 --temporal-script-position > profile.json
jq --arg model /absolute/path/to/model.onnx --slurpfile profile profile.json '.inference={model_path:$model,profile:$profile[0]}' capture-options.json > inference-options.json
```

```sh
pulsar live-capture start PROJECT_ID REVISION inference-options.json --request-id START_ID
pulsar live-capture status PROJECT_ID REVISION CAPTURE_EPOCH
pulsar live-capture push PROJECT_ID REVISION CAPTURE_EPOCH frame.json frame.rgb
pulsar live-capture stop PROJECT_ID REVISION CAPTURE_EPOCH --request-id STOP_ID
```

Start first commits its exact request receipt and publishes a bounded lease with inference state `starting`. Only afterward does an engine-owned thread reserve model-worker resources, capture exact worker/model/runtime bytes and wait for readiness outside authority/database locks. Successful readiness changes that same attempt to `running`; failure retires the capture with `worker_failed`, a bounded typed error and no latest estimate. Start replay reports current or retired state without creating another worker. Failed receipt insertion publishes no lease or worker. Platform/runtime limits disclose whether the implemented continuous CPU adapter is available; this is separate from qualification.

The analysis thread owns the persistent worker and uses only a weak engine reference between authority operations. Entry retirement/drop permanently signals cancellation. The loop sweeps revision/grant/expiry invalidation, takes opaque latest-pair work under authority locks, drops those locks for native inference, then rechecks authority and exact identities before completion. Source/capture gaps reseed pairing, pending replacement does not accumulate a backlog and issued bytes retain their original identity. The nominal five-millisecond idle poll is subject to host/DB scheduling and is not a qualified real-time guarantee.

Before accepting a result the engine independently checks worker attempt, capture/project/revision/source, complete profile digest, original pair declaration and exact pixel hashes. The pixel runtime then checks the sealed work owner and capture age against the admitted policy using the server's monotonic lease clock. Stale completion increments the stale counter without publishing an estimate. Stop, expiry, revision changes and lost grants discard issued results and clear the latest estimate. A mismatched result retires the lease immediately. Worker failure retirement does not require a healthy project database. No old epoch is rearmed.

Reports retain only one latest accepted result, completed/accepted/stale counters and bounded queue observations. `capture_to_analysis_ns` describes age when that result was accepted. `capture_age_ns` and `fresh` are recomputed when the report is read, so an earlier accepted estimate ages out even while no new frames arrive. Retirement clears it. Source timestamps and producer capture ticks remain declarations; server age accounting does not calibrate a camera/player clock or measure physical response. Pair analysis duration is distinct from capture age and presentation lateness.

The report explicitly marks `evidence: inferred` and `qualified: false`, with exact profile/pixel/source/presentation correspondence and a normalized position and the original capture/revision. It does not publish a durable per-frame receipt, commit a candidate, buffer a player, arm a device or dispatch Handy commands. Snapshot/model admission remains separate from the 61,440-byte retained pixel reservation. Absolute lease/worker lifetime is at most thirty seconds; renewal and continuity across actual running sources remain required successors.

Native authority regressions cover accepted-result ageing, stale completion, issued-result discard after revocation/revision changes, and wrong-attempt retirement. The actual CLI regression starts an explicit authored model, waits for readiness, sends four chronological frames, observes three model results in one attempt, verifies freshness expiry, replays start without rearm, stops and exercises asynchronous bad-byte failure. Those fixtures establish local software integration, not model, media or hardware qualification. Training weights and the prior negative sterile campaign are unchanged.

The retained [validation checkpoint](evidence/roadmap-2026-10-01/authenticated-live-inference/validation.json) binds the final production/verification source and native binaries to 735 passing Rust tests, seven ignored qualification cases, all 75 passing native-configured JavaScript cases without skips and 175 passing structural checks. Native build and diff checks passed. Eighty-three CLI capture traces retain the actual composed path, including nine accepted-result status observations that show explicitly inferred, unqualified estimates ageing from fresh to stale. The checkpoint records historical intermediate logs separately. Acquisition, measured synchronization, player buffering, model quality, GPU six-axis capability and Handy/physical/platform/human/release qualification remain open.
