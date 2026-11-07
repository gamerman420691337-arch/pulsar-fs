# ADR B5-013: Native delivery clock and absolute lease boundary

Status: private clock/lifetime adapter implemented; authenticated source integration open.

## Decision

Measure parent send/Ready receipt around the worker's own monotonic Ready sample. Under an explicit equal-rate process-local assumption, preserve the full readiness uncertainty and map later delivery to an interval. Keep native stdout delivery distinct from exact source PTS and unknown exposure/source buffering. Use the earliest bound for conservative delivery age; never stamp current consumer time onto an older decoded frame. Require a bounded consistent interval strictly before expiry and causally consistent with actual packet receipt.

Make validated source frames immutable runtime handles with private facts/pixels and process-local origin. Archive facts with full attempt/capture identity and exact worker/tool/pixel pins, but do not deserialize them into measured handles or authority. Reject archive claims of physical exposure/qualification.

Add an existing-lease launch API whose effective deadline is the minimum of caller absolute end and manifest budget. Do not reset the lease clock during staging/readiness/transport. Reject invalid/expired lifetimes before launch and keep independent native supervision on the same end. Share lifetime admission with persistent inference and pass the actual capture entry origin/end from authenticated automatic analysis.

## Successor

Authenticated encoded transport must enforce fresh actor/revision/boot/lease authority, bind the actual entry origin/end, publish only validated frames and propagate the explicit delivery age basis into capture/inference reports. Source continuity, measured device/player clocks and physical scheduling remain separate work. Native acquisition remains unadvertised.

See [native delivery clocks](../../docs/PULSAR_NATIVE_DELIVERY_CLOCKS.md) for pure/protocol/runtime and actual confined native evidence. Authored correspondence does not establish real acquisition/model/Handy/platform/human/release qualification. Training and sterile evaluation were unchanged.
