# B3-018: Worker-side CUDA SDK identity

Status: implemented prerequisite; production CUDA sessions and admission remain open.

Kernel node minors and held mounted inodes do not identify CUDA's logical ordinal. A native worker must query its selected driver before executing on the requested device, rather than infer ordinal identity from kernel metadata or a parent `nvidia-smi` process.

The worker loads an explicit captured driver through a nofollow regular-file descriptor retained with the library. It checks SDK UUID, canonical PCI address and ordinal, plus bounded inventory, total-memory and API-version facts. Rechecking refuses changed initial facts. Native loading stays in the worker; default engine admission continues to refuse CUDA. The caller owns capture authorization, confinement, deadlines and reservation lifetime.

The [contract and evidence](../../docs/PULSAR_CUDA_SDK_IDENTITY.md) record host checks and refusal cases. Host SDK consistency is not confined Rust inference, authorization, reset exclusion, enforceable VRAM capacity or physical/model/platform qualification. Differing SDK and `nvidia-smi` total-memory observations remain distinct. Future production integration must retain these distinctions and bind the SDK object to the actual session lifetime and durable attempt.

Rollback: remove the unused SDK helper/example and its Linux-only direct dependency edge. No durable database/profile change or enabled production GPU path needs migration.
