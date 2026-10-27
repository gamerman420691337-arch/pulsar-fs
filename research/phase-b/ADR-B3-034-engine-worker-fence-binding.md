# ADR B3-034: bind ordinary worker ownership before admission

Status: ordinary Linux worker integration implemented; full lifecycle qualification incomplete.

Own the lease in the engine and transfer it into ordinary generation, resume and preview custody. Bind its canonical scope in the same generation transaction before dispatch, or in a fresh authorized preview transaction before launch. Use conditional schema 17 to refuse older database readers; preserve CUDA intent/evidence layouts and the existing portable format.

Read the binding before writable SQLite initialization. A bound database cannot recover missing lease/roster files by minting a new scope. Startup observes the entire roster and refuses unresolved trees without signaling them. Dispatch rechecks the scope before running queued work.

Hold the actual cgroup directory for signals and observations. An old owner must not operate on a replacement occupying its previous pathname. Directory removal also checks identity. Keep cancellation and deadline checks before spawn after synchronized registration.

Native kernel, engine and reader cases establish their declared local scenarios. The host denied populated-group rename, so the replacement case uses actual permitted empty-group removal/recreation. Full live/GPU lifetimes, directory reclamation, power loss, blocking IO, model/media/device/platform/human and release qualification remain separate obligations. See the [contract](../../docs/PULSAR_ENGINE_WORKER_FENCE.md).
