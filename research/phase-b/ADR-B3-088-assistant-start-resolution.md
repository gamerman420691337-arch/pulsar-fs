# ADR B3-088: resolve accepted assistant starts or withdraw unpublished IDs

Status: implemented software increment; qualification incomplete.

An exact saved Start can lose its acknowledgement before the client learns an attempt ID. Retrying alone leaves an unpublished request indefinitely uncertain. Provide an explicit authenticated resolution command that returns the accepted original attempt or permanently withdraws the unused original request namespace.

Require current Read/Edit, the captured revision's existence and the exact full invocation. Validate accepted attempts against the stored authenticated Start fingerprint. For absence, reserve the actor's original namespace with a distinct domain-separated fingerprint and persist the resolution receipt in the same immediate transaction. Refuse occupied namespaces. Never dispatch inference or signal workers from resolution. Stable first-party resolution IDs replay immutable receipts; status refreshes accepted state separately.

Permit historical private-intent restore for resolution while retaining current-revision and inference-approval requirements for Start. Explain withdrawal explicitly in CLI and desktop controls. Preserve pending cancellation identity and saved files. Retention, corrupt-cell bounds, controlled concurrency/formal checks, durable cancellation, effects/history and delivery/qualification remain independent work. See [behavior and evidence](../../docs/PULSAR_ASSISTANT_START_RESOLUTION.md).
