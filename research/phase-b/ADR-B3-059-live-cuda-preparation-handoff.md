# ADR B3-059: Retire preparation CPU admission before GPU live startup

Status: Private handoff, native execution and final serial regression campaign verified. Public GPU live remains disabled.

The Engine defaults to two worker slots. Retaining a preparation worker slot through the entire GPU stream consumes one slot for an already-completed task and another for inference, preventing coexistence with an encoded-source or control worker. Increasing the default slot limit would not establish correct ownership.

Split snapshot storage from the preparation CPU reservation without changing aggregate charges. Retain both claims through staging creation and failure cleanup. Return successful preparation only after observing and joining its producer's exit under the original lease. Add a private consuming `Prepared::into_gpu` transition that retires CPU RAM/worker admission and produces `GpuPrepared`, the required input to private live GPU launch. Ordinary CPU launch keeps its existing worker admission. Staging retains storage independently until confirmed cleanup.

A channel reply is not evidence of thread exit. On deadline/cancellation after a reply, move the handle and result into independent retirement custody rather than block the caller or drop claims prematurely. Preserve custody if a retirement thread cannot start. Test the producer epilogue with a compile-time-only blocking hook.

Verify the concrete capacity requirement by holding another worker reservation throughout native Engine startup, three exchanges and Finish under the default two-slot pool. Independently verify original native evidence. A reserved accounting slot is not proof of a simultaneous decoder/control transport or a qualified latency distribution. The public registry lifecycle and all model/media/device/platform/human acceptance requirements remain separate. See [implementation and evidence](../../docs/PULSAR_LIVE_CUDA_PREPARATION_HANDOFF.md).
