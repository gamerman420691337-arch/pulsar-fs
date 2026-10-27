# Generation analysis dimensions

New detector generation requests default to the declared model's input width and height. This avoids shrinking media to the old 320 by 180 limit before expanding it for a larger model. Flat video keeps its aspect ratio within the limit and does not upscale in the decode step; declared detector preprocessing still fits the model tensor separately.

CLI `generate` and `stash generate-missing` accept optional `--width` and `--height` overrides. An omitted side uses its model default. With no model the default remains 320 by 180. Temporal script-position models retain 64 by 64 and reject conflicting CLI dimensions. For custom models without a profile, the dimensions also declare the tensor shape, as in ordinary CLI; the bare custom Stash tensor default is now 640 by 640. Declare a profile or both dimensions if the supplier requires another shape.

The desktop Generator displays the effective generation analysis limit and provides an optional custom size for ordinary and Stash generation. Temporal models disable this override. Preview retains its independent limit. Recorded generation and archived Stash batch settings are not rewritten, and their explicit dimensions remain unchanged.

## Verification

Exact subjects and checks are recorded in `evidence/roadmap-2026-10-11/analysis-dimensions/checkpoint.json`. Tests cover declared rectangular dimensions, partial overrides, omission in both CLI paths, no-model and temporal defaults, and invalid dimensions. The native process campaign uses the pinned official generic COCO80 model with explicit letterbox, the pinned CPU runtime and four identical public bus frames in an offline private namespace. Actual broker receipts verify both requested limits and decoded dimensions, source dimensions and original frame ordinals. Invalid dimensions must refuse before any new job exists. The preview pixels and constant default export are also checked.

For the actual 810 by 1080 source:

| Request | Requested limit | Actual decoded view |
| --- | --- | --- |
| No size flags | 640 by 640 | 480 by 640 |
| Explicit width 320, height 180 | 320 by 180 | 135 by 180 |
| Width 320 only | 320 by 640 | 320 by 426 |
| Height 180 only | 640 by 180 | 135 by 180 |

All four jobs completed with original frame ordinals 0 through 3 and project times 0, 0.5, 1 and 1.5 seconds. Width zero and height 16385 returned `InvalidRequest: dimensions must be within 1..=16384`, without adding jobs. Preview remained 360 by 480 with five observations. The default export retained four constant actions at position 50; its bytes match the prior explicit 640 by 640 campaign. This is not a comparison of target quality across sizes.

This is request and decode correctness evidence on an ordinary public image. Larger frames do not establish useful target-trained quality, calibrated scores, preset qualification, speed or memory limits across real media, native GUI acceptance, physical Handy behavior or release readiness. No annotations were supplied. Full roadmap status remains PARTIAL.
