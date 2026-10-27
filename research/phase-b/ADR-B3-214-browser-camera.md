# ADR B3-214 — Browser camera presentation provenance

Date: 2026-10-16
Architecture: ARCH161

Camera input uses the existing consent-based browser stream owner and shared RGB
live adapter. A separate `camera_capture_presentation` declaration survives
continuation, recording and durable source proof. Treating it as source-media time
would give exported motion an unsupported synchronization claim; treating it as
tab time would erase which acquisition path supplied the frames. Neither domain
is camera exposure calibration. Assembly refuses mixed domains and camera
candidates carry the existing capture-relative timing warning.

One pending camera/tab selector, generation checks and track ownership prevent
late consent or obsolete track events from replacing current input. Acquisition
requests no audio. No new generator, device authority or model default is added.
See the [contract and verification](../../docs/PULSAR_BROWSER_CAMERA.md).
