# ADR B3-030: one parent deadline for ordinary worker pipes

Status: implemented for ordinary Linux worker pipes; complete launch retirement remains open.

The prior launcher wrote the request before creating its wall deadline. It read artifacts and diagnostics on separate threads and joined them after worker exit or cancellation. A descendant retaining a pipe could outlive the deadline and prevent those joins from returning. Diagnostics also stopped draining after the retained 16 KiB, which could block a worker writing more stderr.

Use nonblocking descriptors and a shared parent monitor for request delivery, response frames/chunks/EOF and exit polling. Drain bounded diagnostic increments while retaining only the first 16 KiB. Avoid reader threads and their joins. Represent cancellation with a non-retryable IO kind; `read_exact` retries `Interrupted`, so that kind cannot carry the cancellation decision. Reject late success after parent output accounting as well.

Keep the existing owned-cgroup kill and child wait on transport errors. This still blocks on process retirement; it is explicitly not proof of full custody or independently bounded parent filesystem IO. Engine GPU admission remains disabled until reservations, durable launch, confirmed retirement and restart fencing are integrated. Portable accepted CUDA evidence remains a separate required closure.

See the [contract](../../docs/PULSAR_WORKER_PIPE_DEADLINES.md) and its source-bound local validation packet. No schema or portable-format version changes are needed for this pipe implementation.
