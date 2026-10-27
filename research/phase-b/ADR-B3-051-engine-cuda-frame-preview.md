# ADR B3-051: Owned interactive CUDA detector frame Preview

Status: Implemented and verified for explicit authored detector frame Preview through the native Engine path. No release, real-media or sustained live qualification is claimed.

Use the same supervised captured-library/device owner for explicit detector frame Preview and generation. Bind the model identity, runtime, captured libraries and exact profile into the Preview request. Commit the existing creation barrier before capture; capture stays outside the database lock and observes the registered Preview cancellation flag. Final authorization and selected-pack correspondence are checked before dispatch and again before returning the frame.

Frame Preview uses interactive GPU admission. The owner reserves a distinct offline allowance; a permitted interactive profile may exceed that offline ceiling. Selecting a pack validates against the interactive ceiling as metadata, while actual generation still checks the offline ceiling and Preview checks interactive admission. Neither selection nor a saved declaration authorizes GPU execution.

Retire the CPU memory observer before fresh GPU admission, transfer the already reserved output-storage charge without release/double counting, and retain the GPU completion claim through frame identity checks. Consume the owned launcher and its existing bounded artifact egress. Detector finalization publishes the original CUDA receipt/trace; the parent reparses those bytes and verifies request/profile/device binding before returning observations. CPU Preview remains on its existing launcher. Transient Preview does not create durable generation jobs or accepted motion.

This changes no client protocol or database schema. Single-frame temporal script-position profiles remain unsupported because they do not produce detector boxes. Automatic GPU recovery, fastest-preset live six-axis output, interactive latency, selected-pack native CUDA correspondence and independent model/media/platform/Handy/human acceptance remain separate obligations. Capturing a fresh image for each requested frame is not evidence of responsive sustained live operation.
