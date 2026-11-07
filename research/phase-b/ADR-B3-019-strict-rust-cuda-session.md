# B3-019: Held SDK identity through strict Rust CUDA sessions

Status: implemented adapter prerequisite; production GPU job admission remains open.

The preceding C++ graph and standalone SDK probe do not establish a selected Rust worker session on the checked device. The worker therefore owns a feature-gated session wrapper with a held SDK object, strict CUDA registration, disabled CPU fallback and identity checks around native work. Returning CPU-owned float copies avoids native values retaining a session beyond the driver object's lifetime. Errors invalidate the session; successful-call counts and final original-byte provider traces are checked before a diagnostic receipt is returned.

The [contract and evidence](../../docs/PULSAR_RUST_CUDA_SESSIONS.md) record actual captured-library namespace computation, numerical/trace oracles, default and CUDA-feature checks, failed-session reuse, wrong-count, preinitialized-runtime, ordinal and CPU-only refusal. The broker remains responsible for authorization, captured identities, reservation, process confinement/deadlines and durable attempt integration. Default production admission stays CPU-only.

The arena limit and tensor copy bounds do not become whole-worker memory quotas. Native mappings and node traces do not qualify arbitrary model/library closure, runtime authenticity, physical reset exclusion, model quality, reference-hardware speed or release acceptance. Those gates remain separate.

Rollback: disable the feature-gated adapter and remove its research examples/cases. No database, profile schema or enabled production GPU path requires migration.
