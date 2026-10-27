# Checked generation detector score

Generation accepts an optional minimum uncalibrated detector score. Omission keeps the actual legacy cutoff of 0.25. The CLI previously advertised 0.35 while the worker used 0.25; that mismatch is removed. Explicit values must be finite and between 0 and 1 inclusive, with a declared detector model/profile. Temporal regression and no-model flow refuse the option.

Use `generate --conf 0.5` with an explicit model or selected pack. Stash batch generation accepts `--detector-score-threshold 0.5`. The desktop Generator provides an optional minimum score for declared detectors and uses it for ordinary and Stash generation. Preview keeps 0.25; the interface states this distinction. The score filters observations before tracking and is not a probability or quality guarantee.

Generation lineage records the effective `detector_score_threshold`, the immutable configuration and its digest. Old omitted settings omit the new configuration property and retain their serialized identity. Explicit settings participate in configuration identity. Raising the threshold can leave unavailable target coverage; standard export continues to refuse unresolved gaps.

## Verification

Evidence is recorded in `evidence/roadmap-2026-10-11/detector-settings/checkpoint.json`. Tests cover legacy serialization, finite/range/model checks, exact threshold boundaries, CLI grammar and desktop request selection. The process campaign uses the pinned official generic COCO80 model, explicit letterbox profile, CPU runtime and four identical public bus frames in an offline private namespace. It compares omitted 0.25, explicit 0.5 and explicit 1.0 generation and reads actual worker lineage. Observed counts on each frame were 5 at 0.25, 4 at 0.5 and 0 at 1.0, with three distinct configuration digests. The 1.0 job completed with unavailable detection/motion review flags and standard export refused unresolved gaps. Default export retained four constant actions at position 50, with zero stroke travel. Preview retained five observations and was checked independently against decoded frame pixels.

These checks establish software plumbing and filtering on an ordinary image. They do not establish target-trained accuracy, script timing, calibrated confidence, useful Fast/Default/Maximum presets, native window or human acceptance, physical Handy behavior or release readiness. No annotations were supplied; software work continues without inventing training labels. Full roadmap status remains PARTIAL.
