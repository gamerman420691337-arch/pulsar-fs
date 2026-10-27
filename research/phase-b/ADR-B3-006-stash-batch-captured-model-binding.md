# ADR B3-006: Bind prepared batch models to original capture

Status: implemented locally; qualification and explicit replacement migration remain open.

The engine's durable Stash batch already captures a model before scheduling, but its internal Generate request only retained that snapshot's pathname. An authored native baseline changes producer metadata in the captured ONNX bytes and still finishes/publishes with a different model identity. Valid weights or equal outputs cannot replace original input identity silently.

Recover the original digest from the engine-issued content-addressed model locator. Bind it to the existing scoped in-memory permit, which already names one thread/session/batch/request/fingerprint. Compare the prepared model artifact's identity after copying and before the authority transaction can publish a generation job/attempt. Refuse a non-issued locator or an identity mismatch. No public authority field, independent generator, model activation or new grant is introduced.

Treat a generation-phase dependency mismatch as attention requiring explicit recovery. Preserve the exact pending command/effect ID. Matching-byte restoration plus ordinary explicit resume is supported; different model/configuration or engine/tool/owner adoption remains a separate migration intent. Failed copying/admission can retain inert artifacts rather than hide deletion or rollback.

Retain schema 9 / portable maximum 7. The existing worker stamp binds the generation engine/tool identity, so the pinned preceding schema-9 engine refuses the new batch before generation despite its weaker model guard. Native tests independently exercise that refusal, matching restoration/restart, exactly one job under the original phase ID and unchanged original suppliers. Two focused Rust tests cover locator and exact prepared-request bindings.

See ../../docs/PULSAR_STASH_BATCH_MODEL_IDENTITY.md and the exact final source/binary receipt. No model-quality, provider, installed Stash, hardware/platform/human or release qualification follows.
