# ADR B3-112: generation target/background flow diagnostics

Status: implemented diagnostic retention; native and qualification evidence remain separate.

Generation previously retained selected identities and review reasons but discarded the checked target/background image translations. A constant export alone cannot distinguish successful compensation from an unsupported flow stage. Retain the bounded MotionSeparation output in each generation sample after the actual separation stage, including accepted point counts and null translations for unavailable or inconsistent support. Omit it when that stage was not reached. Serialization is output-only; this does not add an unvalidated input authority.

The receipt states the coordinate basis: decoded analysis-view pixel translation, not physical camera pose, metric target movement or calibrated six-axis motion. The existing inferred stroke, synthesized anchor and unavailable gap semantics remain unchanged. No observation, track or confidence becomes a physical-motion claim. Configuration identity is unchanged; receipt identity changes with the added diagnostic evidence and is handled through existing artifact custody.

The controlled native campaign translates the pinned public image as a whole, preserving a known absence of object-relative movement. Run it at the new model-sized default and the legacy small explicit limit. Inspect actual model observations, selected track continuity, target/background support, image-relative subtraction, original clocks and exported motion or gaps. Public-image transformation is a numerical software fixture, not real-media target or device qualification.

See [receipt semantics and scoped results](../../docs/PULSAR_CAMERA_MOTION_EVIDENCE.md). Full target quality, large/fast camera motion, independent object motion, cuts, occlusion, performance and release/device/human gates remain open.
