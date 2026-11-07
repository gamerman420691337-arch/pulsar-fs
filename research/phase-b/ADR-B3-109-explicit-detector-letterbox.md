# ADR B3-109: explicit fixed detector letterbox

Status: implemented software; numerical and product qualification recorded separately.

The declared profile already has a `letterbox` variant, but adapter admission previously refused it. Implement a fixed centered RGB byte-space linear recipe in the isolated detector adapter. The profile supplies border value and tensor dimensions. Legacy `stretch` keeps its integer-floor nearest recipe; it does not silently become letterbox. The CLI adds explicit `model profile --letterbox` and optional `--padding-byte`, mutually exclusive with temporal/annotated temporal recipes. This is an additive client interface; the authenticated wire schema is unchanged.

Use ties-to-even resized dimensions, fixed target shape, no automatic stride reduction, scale-up allowed and centered integer borders with any odd extra pixel on the right/bottom. Refuse resized dimensions that round to zero. Byte-space sampling uses half-pixel coordinates, 11-bit separable weights, checked byte conversion and replicated edges. Normalize only after byte sampling. The exact plan determines source-pixel inversion through the shared checked DetectorTransform; normalize resulting observations to the decoded source view. Rounded resized dimensions define actual per-axis scale. Padding-only boxes disappear after clipping. Existing selected-eye/projection/PTS and model/profile identity bindings remain required.

Preprocessing, native execution and decoding retain one failed-session scope. Admission checks only implemented RGB/NCHW/YOLO channel-major normalization; temporal, BGR and unsupported decoder/layout/scale combinations still refuse. Supporting shared CUDA preprocessing does not qualify CUDA execution. No profile generation, supplier import or model selection becomes release qualification.

See [the recipe and exact evidence](../../docs/PULSAR_DETECTOR_LETTERBOX.md). Pixel correspondence uses independently generated pinned OpenCV/Ultralytics fixtures; useful target/script quality, performance, default packs, platforms, physical devices and human acceptance remain separate.
