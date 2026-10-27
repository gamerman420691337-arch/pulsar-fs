# Generation flow diagnostics and camera-motion fixture

Generation receipts now retain an optional `motion_separation` for each sample that reaches target/background separation. It contains target image translation, background image translation, target relative to background, and accepted point counts. The translations use decoded analysis-view pixels. A null translation means the corresponding region lacked enough consistent support; an absent diagnostic means separation was not reached. These are image-motion estimates, not measured physical camera pose or anatomical movement.

The existing tracking and stroke rules are unchanged: independent target/background support is required for subtraction; a first target anchor is synthesized; relative image stroke is inferred; unavailable intervals remain gaps. The new output-only serialization does not admit client-authored flow evidence or alter configuration identity. Receipt hashes continue to bind the actual diagnostic content.

## Controlled native exercise

The fixture uses the pinned official generic COCO80 model, explicit letterbox, pinned CPU runtime and public 810 by 1080 bus image. Four frames shift the whole image down by 0, 6, 12 and 18 original pixels, filling exposed top rows with RGB 114. There is no independently moving object. Encode at two frames per second and run actual CLI, Engine and confined worker generation at the automatic 640 by 640 limit and explicit 320 by 180 limit. Inspect actual broker receipts and standard exports; retain null support and review flags rather than claiming compensation from an output file alone.

Exact subjects and results are recorded in `evidence/roadmap-2026-10-11/camera-motion/checkpoint.json`. The public transformed clip and receipts are retained separately from private Engine state. Native GUI/human acceptance, target-trained accuracy, wide camera/model/media coverage, large motion, independent target motion, real-media timing and performance, physical Handy behavior and release qualification are not established by this fixture. Full roadmap status remains PARTIAL.


## Observed result

The actual native comparison passed all six frame pairs. Both jobs completed with one continuous selected identity, a synthesized first anchor and three inferred relative-image samples. Both standard exports retained four actions at position 50, with zero integer stroke travel.

| Decoded view | Expected vertical image translation per pair | Accepted target points | Accepted background points | Maximum pixel error |
| --- | --- | --- | --- | --- |
| Automatic 480 by 640 | 3.555556 | 3 to 4 | 20 to 23 | 0.044939111 |
| Explicit small 135 by 180 | 1 | 12 | 13 to 15 | 0 |

The predeclared comparison tolerance was 0.5 analysis-view pixels. Error includes target/background horizontal drift, their vertical error against the independently known source shift, and the image-relative subtraction. This single fixture does not establish calibration or a useful physical target model. Source frame ordinals and the two-frame-per-second project clock were preserved. The baseline static preview and export checks also passed.
