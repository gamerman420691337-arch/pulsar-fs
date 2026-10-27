# Historical twenty-regression execution receipt

Isolated historical experiment, not B1 completion or production qualification.

- State: blocked_before_all_original_tests_executed
- Cargo exit: 101
- Executed: 0/20; passed: 0; failed: 0; unavailable: 20.
- Preserved baseline unchanged: true; original fixture unchanged: true.

| Original test identity | Actual result | Related current-contract evidence |
| --- | --- | --- |
| neural::yolo::m1_regression_tests::m1_reject_negative_box_extent | unavailable_build_or_harness | Declared tensor geometry; category correspondence, not identical legacy API. |
| neural::yolo::m1_regression_tests::m1_reject_nonfinite_geometry_and_scores | unavailable_build_or_harness | Declared tensor geometry; category correspondence, not identical legacy API. |
| neural::yolo::m1_regression_tests::m1_clipped_box_center_matches_visible_bounds | unavailable_build_or_harness | Declared tensor geometry; category correspondence, not identical legacy API. |
| neural::yolo::m1_regression_tests::m1_unknown_class_does_not_panic | unavailable_build_or_harness | Declared tensor geometry; category correspondence, not identical legacy API. |
| neural::yolo::m1_regression_tests::m1_zero_classes_cannot_create_detection | unavailable_build_or_harness | Declared tensor geometry; category correspondence, not identical legacy API. |
| neural::yolo26::m1_regression_tests::m1_direct_rejects_reversed_corners | unavailable_build_or_harness | Explicit pixel-corner layout differs from old normalized direct-output API. |
| neural::yolo26::m1_regression_tests::m1_direct_clips_corners_without_guessing_center_format | unavailable_build_or_harness | Explicit pixel-corner layout differs from old normalized direct-output API. |
| neural::yolo26::m1_regression_tests::m1_direct_rejects_nonfinite_score | unavailable_build_or_harness | Explicit pixel-corner layout differs from old normalized direct-output API. |
| neural::tracker::m1_regression_tests::m1_flow_advances_box_and_center_together | unavailable_build_or_harness | Velocity prediction is not historical dense-flow box propagation. |
| neural::tracker::m1_regression_tests::m1_scene_cut_clears_old_targets_and_points | unavailable_build_or_harness | Velocity prediction is not historical dense-flow box propagation. |
| neural::tracker::m1_regression_tests::m1_confidence_change_does_not_replace_spatial_match | unavailable_build_or_harness | Velocity prediction is not historical dense-flow box propagation. |
| neural::tracker::m1_regression_tests::m1_missing_detection_ages_without_flow | unavailable_build_or_harness | Velocity prediction is not historical dense-flow box propagation. |
| neural::point_tracker::m1_regression_tests::m1_degenerate_boxes_do_not_seed_points | unavailable_build_or_harness | Independently reversed images replace a single-forward cycle-error oracle. |
| neural::point_tracker::m1_regression_tests::m1_zero_requested_mask_points_do_not_panic | unavailable_build_or_harness | Independently reversed images replace a single-forward cycle-error oracle. |
| neural::point_tracker::m1_regression_tests::m1_forward_flow_is_not_its_own_backward_flow | unavailable_build_or_harness | Independently reversed images replace a single-forward cycle-error oracle. |
| signal::m1_regression_tests::m1_constant_displacement_never_invents_stroke | unavailable_build_or_harness | Earlier actual-worker synthetic hold/direction/plateau/pause-anchor checks; exact current names not asserted or rerun here. |
| signal::m1_regression_tests::m1_monotonic_decreasing_motion_keeps_direction | unavailable_build_or_harness | Earlier actual-worker synthetic hold/direction/plateau/pause-anchor checks; exact current names not asserted or rerun here. |
| signal::m1_regression_tests::m1_trailing_pause_does_not_add_reversal | unavailable_build_or_harness | Earlier actual-worker synthetic hold/direction/plateau/pause-anchor checks; exact current names not asserted or rerun here. |
| signal::m1_regression_tests::m1_keyframe_reduction_preserves_both_pause_boundaries | unavailable_build_or_harness | Earlier actual-worker synthetic hold/direction/plateau/pause-anchor checks; exact current names not asserted or rerun here. |
| tracking::m1_regression_tests::m1_equal_area_resize_updates_flow_geometry | unavailable_build_or_harness | Explicit buffer geometry replaces mutable legacy flow geometry; category correspondence only. |

Exact subject/test hashes and manifest deltas: subject.json. Process outputs: cargo.stdout.log and cargo.stderr.log.
An original failing assertion is recorded, not repaired or relabelled as a passing suite.
Current tests named in results.json are related category evidence and were not rerun.

- Full preserved production files unchanged before exact appended test modules; no extracted convenient functions or stubbed implementation.
- Experimental isolation manifest differs from historical default build; dependency differences and hashes are retained.
- No original legacy Cargo.lock was preserved; generated offline lock is not a recovered historical lock.
- Current-contract relationships are category evidence, not rerun or one-to-one closure.
- Historical single-forward zero-cycle oracle is semantically superseded by independently solved reverse flow.
- No model/runtime inference, private media, GPU, device actuation, or B1 completion is claimed.
