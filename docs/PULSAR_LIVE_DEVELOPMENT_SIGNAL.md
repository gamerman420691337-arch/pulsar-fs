# Live-development position signal baseline

A protocol frozen before measurement tested one fixed retrieval descriptor on the existing live-development dataset. This comparison uses the six optimizer groups (2,280 examples) and two previously consumed development groups (580 examples), with the frozen six-group learned candidate `40f5bcb19c3e4084b809f3c254f023a076bd66bad386a28f5038eaa0e9886225`. This is distinct from the earlier all-group candidate used in the sterile diagnosis. No sterile inputs, fitting, model export, activation or label/alignment changes occurred.

Each previous/current RGB pair is represented by per-channel means over nonoverlapping 8×8 blocks, retaining an 8×8 spatial grid. Euclidean nearest neighbor uses only optimizer rows, with original row order resolving ties. Features, neighbor count and split were not selected using these results.

| Development position metric | MAE |
| --- | ---: |
| Constant optimizer-label mean | 0.257179 |
| Constant optimizer-label median | 0.257090 |
| Frozen learned candidate | 0.285795 |
| Fixed cross-group image retrieval | 0.326741 |

Retrieval is worse than the constant mean in both development groups. The frozen model measurement reproduces its earlier negative result. An optimizer other-group control is also worse than its constant mean (0.260843 versus 0.218613). A same-declared-source control, excluding every neighbor less than one exact source second away, improves to 0.187665. That is useful within-source correlation for this descriptor, not generalization or target identity. It does not prove the task intrinsically impossible or select a replacement model.

The 2,860 original temporal input pairs have no exact duplicate groups, no cross-split exact duplicates and no shared original source hashes across the split. This provides no positive verification of label alignment, near-duplicate independence or source-family separation. Supplied script positions remain unverified physical labels.

Two fixed runs produce identical metrics. Independent readback recomputes MAE from private per-sample predictions, verifies optimizer/development membership, and checks five neighbors using a separate block construction and direct squared-distance formula. The protocol, input/model receipts, cached runtime and private prediction identities are retained in [the checkpoint](evidence/roadmap-2026-10-08/live-development-signal/checkpoint.json). All 367 preceding source files are unchanged; Cargo gates were not rerun. CPU libraries were configured to four threads each; this is not an OS-enforced resource quota.

The successor [label and sampling audit](PULSAR_LIVE_LABEL_CONSISTENCY.md) reconstructs stored labels exactly against their declared script clocks and measures between-frame excursions. Target/pose, pixel-clock correspondence and independently verified visual/script alignment remain open, with this consumed data status retained. An independent acceptance set must be separately justified. This baseline does not establish stroke quality, natural-media perception, model calibration, Handy, platform, human or release qualification. Phase B and the roadmap remain open.
