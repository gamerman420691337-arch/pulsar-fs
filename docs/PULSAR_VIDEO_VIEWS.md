# Declared stereo and spherical video views

Pulsar preview and stroke generation can use an explicitly selected eye and perspective viewport. Source layout, eye order and projection are declared; they are not inferred from the filename or image dimensions.

The current adapter accepts flat packed stereo, full equirectangular 360 eyes and half-equirectangular VR180 eyes. Full equirectangular eyes must have a 2:1 aspect ratio; half-equirectangular eyes must be square. Stereo dimensions must split evenly. Fisheye, cubemap, padded layouts, stereo disparity and physical six-axis reconstruction require separate implementation and evidence.

For CLI generation, save a declaration such as:

```json
{
  "layout": "side_by_side_left_right",
  "eye": "left",
  "projection": {
    "kind": "equirectangular360",
    "viewport": {
      "yaw_degrees": 0,
      "pitch_degrees": 0,
      "roll_degrees": 0,
      "horizontal_fov_degrees": 90,
      "vertical_fov_degrees": 90
    }
  }
}
```

```sh
pulsar generate video.mkv --video-view view.json --width 320 --height 180 --output video.funscript
```

An explicit trained model still requires its declared model/input profile. Without a model, the existing global image-motion recipe remains unqualified and retains its uncertainty. Unresolved motion gaps continue to block standard funscript export.

Layouts are `mono`, `side_by_side_left_right`, `side_by_side_right_left`, `top_bottom_left_right` and `top_bottom_right_left`. Eyes are `left` and `right`; mono uses the single `left` convention. Projection kinds are `flat` (no viewport field), `equirectangular360` and `half_equirectangular180`. The legacy `--vr` switch remains unsupported because it does not specify these facts. The protocol's legacy VR marker requires an explicit view declaration; it does not select an adapter by itself.

Yaw/roll are bounded to -180..180 degrees, pitch to -90..90, and perspective fields of view to 1..170 degrees. All values must be finite. VR180 viewport coverage must stay within a conservative front-hemisphere bound. A view outside that domain is rejected rather than filling missing source coverage and presenting it as available input.

In the desktop Generator, enable **Declare stereo / spherical source view**, then select layout, eye and projection. Spherical sources expose yaw, pitch, roll and field-of-view controls. Preview and generation use the same declaration; changing it clears the prior preview. Desktop Stash batch generation uses the current view settings. CLI Stash batches accept the same `--video-view FILE.json` option. Their durable ledger retains the declaration; invalid declarations are rejected before discovery.

Desktop seeks use [explicit elapsed/source clocks](PULSAR_PREVIEW_CLOCKS.md), retaining the actual source PTS and frame ordinal even for a nonzero clip origin.

Preview observations are normalized to the displayed selected view. The worker records its transform identity, exact declaration, original source dimensions, analysis dimensions and original frame/PTS mapping. It does not retime source timestamps, map boxes back into the original packed source, reconstruct stereo depth or qualify model accuracy. Checked point maps now accompany explicit-view previews and generation receipts. Coordinates use normalized image edges, with pixel centers at `(i + 0.5) / size`. `ViewCoordinateMap.to_source` returns nominal packed-source sampling centers; `to_view` returns no point for another eye, outside the spherical sample domain or viewport, behind the camera, or at a pole with ambiguous longitude. Full-360 seams retain equivalent source rays. Desktop preview displays the mapped source position under the cursor. Explicit-view preview matching refuses a missing map, an inconsistent transform or a map whose output dimensions do not match the frame. These maps do not infer rectangular box bounds, stereo depth or project source placement. Source-placement and broader seek/format/platform evidence remain explicit GB-03 obligations.

See [ADR B4-001](../research/phase-b/ADR-B4-001-declared-selected-eye-views.md) for the fixed worker adapter, synthetic fixture protocol and remaining qualification boundary. [Current validation](evidence/roadmap-2026-10-01/validation.json) binds local checks to their actual source and binary subjects; it does not establish real projected-media quality or interactive desktop usability.
