# Declared live timestamp domains

LiveCaptureOptions and Engine-owned recording pages now carry source_time_basis:
producer_declared (legacy/default unknown declaration), source_media_presentation
(local video presentation time), or display_capture_presentation (shared-stream
presentation time). None is clock calibration or physical synchronization evidence.
Absent legacy fields remain producer-declared and omit the new field on serialization.

The browser declares local versus shared-stream input at Start. The Engine retains
that admission label in capture reports and recording pages; continuation cannot
change it. Assembly refuses pages from mixed domains. Durable recording source proof
includes the label, and display-capture candidates receive live_capture_time_not_media_time
review diagnostics. Archive pagination includes it in stable metadata validation.
Encoded-source capture cannot claim display-capture time.

This preserves provenance without assigning tab-player identity, detecting its seeks,
or converting capture time into source-video time. Exported funscript timestamps still
use the declared project mapping; a capture-relative recording must not be interpreted
as a synchronized script for the original tab video. Useful model, browser/platform,
source-player clock and physical Handy/human/release gates remain open.

Verification: [checkpoint](evidence/roadmap-2026-10-13/live-time-basis/checkpoint.json).
Full roadmap PARTIAL.

## Scoped verification

Engine library tests pass 882 with 70 ignored; Protocol passes 298 with none ignored,
Clients pass 425 with ten ignored, and 24 player mocks pass. Strict Client and workspace/
all-feature lint, App/Worker builds, formatting, architecture and guards pass. The new
Protocol test rejects mixed recording domains and verifies explicit/legacy serialization.
Native real CPU/HTTP Start declares display-capture presentation, confirms that basis in
Engine recording pages and observes the candidate timing warning. Three-scope prepared
continuation preserves the label; existing two-cycle record/repair/export and index
retirement/restart checks pass with empty custody and joined owned processes.

The first broad check found one root process-test constructor missing the new field;
its compatibility default was added. The first Engine suite lacked a declared ONNX
runtime and two unchanged RAM-profile fixtures refused setup. Its failure is retained;
full Engine verification then passed with the exact borrowed runtime supplied. These
are scoped native producer declarations, not real browser capture or clock calibration.
Core source is unchanged; no fresh Core suite is claimed. Broader clock/domain faults,
browser/platform, useful-model/media and physical-device/human/release gates remain open.


Camera input adds a separate `camera_capture_presentation` declaration with the same
capture-relative review boundary. Camera/tab/source-media domains cannot be mixed
in assembly. See [camera input](PULSAR_BROWSER_CAMERA.md) for its current contract
and verification scope.
