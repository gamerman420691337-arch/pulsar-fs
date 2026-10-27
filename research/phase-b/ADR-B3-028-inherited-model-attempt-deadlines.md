# ADR B3-028: Inherited model attempt deadlines

Status: Implemented shared worker deadline propagation; GPU scheduling remains disabled.

The CUDA factory previously started another full wall-time budget for model and library hashing. This could outlive the original worker budget, even though parent supervision and final worker validation still refused late results. A verified loader transition must not accidentally reset its deadline in a nested adapter.

Pass the existing monotonic deadline explicitly through detector, temporal generation and raw-pair factories. Store it on the shared session and check initialization, adapter execution and evidence finalization boundaries. Reject and poison expired execution rather than attaching successful CUDA evidence. Preserve CPU-only ordinary admission, the production GPU guard and the independently supervised live-session contract.

Resetting a nested relative budget was rejected because it changes the attempt boundary. Assuming native calls are interruptible was rejected because deadline checks do not provide preemption. Thread-local deadline authority was rejected in favor of explicit arguments at the actual generation and preview consumers.

The regression reaches the production temporal factory with an expired deadline and verifies refusal before native loading. Fresh-process authored CPU checks separately exercise successful inference and expired run/finalization refusal. Rebuilt confined CUDA generation and the surrounding application regressions remain required. Exact source subjects, helper failures and terminal checks are retained with the [deadline contract](../../docs/PULSAR_MODEL_ATTEMPT_DEADLINE.md).

There is no pre-change native timing baseline; the original reset is directly visible in retained source. This change does not establish native preemption, independent parent IO deadlines, aggregate GPU admission, durable result acceptance, restart custody or model/device/release qualification.

Rollback: revert the propagation and session checks while preserving the existing parent timeout and disabled production GPU scheduling. Retain all evidence; no database schema change or project rollback is required.
