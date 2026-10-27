# ADR B4-001: declared selected-eye analysis views

Status: implemented adapter; broader GB-03 qualification remains incomplete
Date: 2026-10-02
Decision source: architecture-first B2/B4, GB-03 and software specification IN-02. This does not reduce the required projection, placement, preview, six-axis or release scope.

## Question and hypothesis

Can explicitly declared stereo, full equirectangular 360 and half-equirectangular VR180 sources reach production preview and stroke generation without guessing eye order, changing source time or confusing a rectified view with original packed-source coordinates?

The falsifiable adapter hypothesis is that a lossless packed-eye color/ramp fixture selects the declared eye, maps front/quarter-turn/seam longitude as declared, retains the exact requested source ordinal/PTS, and records a matching transform in generation receipts. This is separate from model accuracy, stereo disparity, physical pose and perceptual quality.

## Primary evidence and selected boundary

[FFmpeg's v360 documentation](https://ffmpeg.org/ffmpeg-filters.html#v360) specifies equirectangular/half-equirectangular inputs, rectilinear output, explicit stereo formats, dimensions, yaw/pitch/roll, rotation order and field of view. [The FFmpeg 7.1 v360 source](https://ffmpeg.org/doxygen/7.1/vf__v360_8c_source.html) shows spherical vector rotation and input sampling. These are source references, not a claim that every installed filter build is equivalent. The locally exercised tool reports `ffmpeg n9.0.2`; production receipts retain the actual pinned FFmpeg/FFprobe artifact identities.

Use a checked `VideoView` in the core: source layout and eye order, selected eye, source projection and optional perspective viewport. A mono source has one eye convention. Packed axes must split evenly. Full equirectangular eyes must be 2:1, and half-equirectangular eyes 1:1. Undeclared fisheye, cubemap, padding and alternative encodings are not inferred. Viewport values must be finite and bounded. VR180 viewports must fit within a conservative front-hemisphere cone bound; sampling outside that source domain is refused instead of inventing available pixels.

The confined worker builds a fixed numeric filter: exact selected-eye crop, then optional `v360` rectification with linear interpolation and explicit `ypr` rotation order. Flat stereo uses the same eye crop followed by aspect-preserving resize. No client-supplied filter graph is executed. Existing immutable resource, source, model, tool, deadline and output admissions still apply.

## Shared contract and production consumers

`GenerationSettings.video_view` and preview commands/manifests carry the same optional declaration. Omission preserves serialized legacy request identity. Normalized-view transform identity is a digest over the versioned declaration; clients, engine and worker share its construction. A preview's `FrameContext.transform` must match its declared view before native execution. Changing desktop layout/eye/viewport clears the displayed frame and increments seek/request generations through the existing freshness path.

CLI generation accepts `--video-view FILE.json`. Desktop controls apply to preview, direct generation and Stash batch generation. CLI Stash batches accept the same view file and reject invalid declarations before discovery. The legacy `--vr` switch remains unsupported because it does not identify layout or projection. It is not silently treated as equirectangular video.

The worker analyzes the decoded selected view. Detections and tracking geometry are local to that view, and preview boxes are normalized to the displayed view. They are not coordinates in the original packed image. Receipt fields retain the view transform, declaration, original dimensions, analysis dimensions, actual source frame/PTS and project-time quantization. Projection does not retime motion or establish physical six-axis reconstruction.

## Experiment and verification contract

Lossless FFV1 RGB fixtures encode longitude and latitude as independent channel ramps, eye identity and frame ordinal in a third channel, and eight frames at 8 fps. Tests are authored before execution with these criteria:

- Central/front and quarter-turn longitude/latitude ramp values within five integer channel levels; eye/frame channel within one level.
- Exact selected eye, including reversed packed ordering; no neighboring-eye contribution across the 360 seam.
- Exact source ordinal 3 and rational source time 3/8 for preview.
- Generation sample 3 remains at 375,000,000 ns, with eight source samples and explicit unqualified evidence.
- Invalid packing, non-finite/bounded viewport values, ambiguous mono eye, wrong spherical aspect and out-of-domain VR180 viewport are refused.
- A mismatched preview transform is refused before native execution.
- The compiled CLI traverses authenticated engine and confined worker with the declaration retained in the immutable checkpoint and generation receipt.

Raw/current-source gates are retained in the roadmap validation receipt. Synthetic adapter success does not qualify trained inference, release speed, real-media distortion or desktop usability.

## Remaining obligations

GB-03 remains incomplete. Checked point maps now implement forward/inverse selected-view geometry with explicit pixel-center conventions, finite validation, selected-eye visibility, seam equivalence and pole refusal. Required source placement and late-result/seek scenarios; independent real projected-media and model-quality comparisons; and the finite supported projection/encoding/platform matrix. Stereo depth and six-axis evidence recipes are separate missing capabilities. Additional source formats require explicit adapters and evidence. Clean-machine platform/dependency qualification remains Phase C.

## Checked coordinate-map increment

The pure core map uses FFmpeg flat-output pixel centers and spherical-input sampling centers separately. Spherical source samples span pixel indices `0..width-1`; packed normalized coordinates therefore include the source half-pixel center offset. Rotation follows explicit `ypr` quaternion order (roll, pitch, then yaw for column vectors), with image Y pointing downward. Flat crop/resize uses the nominal affine image-edge map; interpolation and border replication are not claimed to be invertible image reconstruction.

Preview receipts carry a checked map, and frame matching checks its declared transform and output dimensions. Generation receipts retain the same map. Before native execution, the new differential fixture criteria are: combined yaw/pitch/roll on full and half equirectangular sources, three interior pixel centers within three RGB ramp levels, exact eye/frame channel, inverse round-trip error below `1e-10`, other-eye invisibility, and refusal of a mismatched map output shape. Core tests also cover seam-crossing view rotations, flat top/bottom packing and invalid deserialized metadata.

Primary formula references: [FFmpeg v360 source](https://ffmpeg.org/doxygen/7.1/vf__v360_8c_source.html), functions `flat_to_xyz`, `xyz_to_equirect`, `xyz_to_hequirect`, and `calculate_rotation`. The native installed decoder differential is required because reference source alone does not establish current binary behavior.
