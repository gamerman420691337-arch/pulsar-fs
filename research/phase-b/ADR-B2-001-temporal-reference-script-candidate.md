# B2-001: explicit temporal reference-script candidate

Status: trained privately and implemented locally; model qualification remains open.

Supplied scripts provide motion-position labels but no anatomical target boxes or independently measured physical motion. Training a script-position regressor is a distinct task from training a detector. The candidate retains that task identity through training receipts, a fixed model decoder, inference profile, worker recipe and uncalibrated review flags. It cannot report invented detector observations or physical tracking qualification.

Training-only manifests and private snapshots retain rights, original bytes, exact selected source clocks and unverified label alignment. Sterile evaluation is separate and never a training input. The first candidate is frozen before evaluation; training loss is not an acceptance gate.

The engine admits explicit model/runtime/preprocessing identities through existing project/job authority. Native sessions stay inside isolated workers. The temporal adapter checks fixed named float32 tensor shapes before inference and validates output range afterward. CPU initialization is shared with the existing detector adapter. Decoder-dependent preprocessing is explicit: detector RGB stretch and six-channel temporal nearest packed-source stretch are distinct declarations.

Temporal generation preserves source sample times and uses inferred, review-flagged output. First-frame anchors are synthesized; unsupported pairs remain gaps. Preview has no detector adapter for this model. Actual independent model/physical/vendor/release qualification remains required. See [the training and runtime contract](../../docs/PULSAR_TEMPORAL_POSITION_TRAINING.md).
