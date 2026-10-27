# B3-020: Owner CUDA startup policy and real-engine preflight

Status: implemented configuration/admission prerequisite; GPU job launch remains disabled.

The engine previously created an empty GPU accounting inventory and had no owner binding for its expected native image. Startup now reads one explicit bounded private owner file, freezes its exact source identity, validates device/accounting/library roles and matches its ONNX identity to the pinned runtime. The same shared resource pool used by the engine receives the configured GPU policy. The authenticated catalog continues to report CPU-only execution.

Generation/preview preflight checks CUDA intent against this owner declaration and priority ceiling. It then refuses unsupported durable GPU launch before reservation or attempt-directory creation. The file configures intent; it does not load libraries, grant device access, reserve hardware or qualify ABI/model/performance. Changing it does not silently update a running engine.

The [contract and evidence](../../docs/PULSAR_CUDA_OWNER_POLICY.md) retain native real-engine/CLI cases, current full checks and the initial unpublished-directory reproduction separately from its correction. Database/profile/wire schemas remain unchanged. Durable GPU image/resource/device transport, supervised capture, strict worker execution, cancellation/restart fencing and model/platform/device/release gates remain open.

Rollback: remove the optional startup policy and metadata/preflight hooks; an unset policy preserves default CPU accounting. No persisted GPU attempts need migration because none can launch.
