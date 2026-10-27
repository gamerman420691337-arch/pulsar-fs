# ADR B3-027: Confined CUDA worker bootstrap

Status: Implemented private worker seam; production GPU scheduling disabled.

## Question and constraints

How can a request-bound captured CUDA image execute through the actual confined worker and parent artifact pipe without enabling ordinary CPU worker admission or borrowing authority from a research probe?

The child must erase inherited descriptors, verify exact request/image/device bindings before native execution, retain one deadline through initialization, preserve confinement across loader selection and refuse CPU substitution. Native effects cannot be rolled back by rejecting output. Production owner/resource/lifecycle admission remains a separate requirement.

## Evidence and alternatives

The original whole-worker regression fails with CUDA status 304. A minimal native probe requires both fixed NVIDIA module-state files; removing the read-only device remount or dropping capabilities alone does not repair initialization. Adding only CPU/NUMA topology metadata does not repair it either. Broad `/sys` exposure is unnecessary.

The captured CUDA provider then fails to locate captured cuBLAS through the initial system loader. Selecting the captured loader fixes search resolution but exposes unresolved CuDNN symbols. ELF inspection confirms that the provider references the symbol, the captured CuDNN exports it, and the provider has no CuDNN `NEEDED` entry. The earlier working native probe explicitly preloaded this library. A declared captured library loaded globally after verification makes the actual worker regression pass.

Changing `LD_LIBRARY_PATH` after process startup cannot establish this startup loader contract. Hardcoding a CUDA-major collection of host preloads would weaken captured-image custody. Loading undeclared dependencies or silently falling back to CPU would violate the request.

## Decision

Add a private two-stage Linux worker role. Verify the namespace witness, then exec the captured loader with its captured search directory and a fully sealed bounded memfd containing the exact request witness and original monotonic deadline. The second stage consumes that relay, clears descriptors, re-verifies custody and loads declared captured CuDNN globally with its handle retained until output completes. Execute the existing generation adapter and emit through the existing artifact stream.

Bind only the two fixed module-state files using held descriptors. Keep ordinary admission CPU-only and preserve the engine's early unsupported GPU scheduling guard. The complete contract and limitations are in [PULSAR_CUDA_WORKER_BOOTSTRAP](../../docs/PULSAR_CUDA_WORKER_BOOTSTRAP.md).

## Validation and consequences

The actual confined authored request must produce the expected motion and original CUDA evidence accepted by the parent reader. Altered request/alias/device identity must refuse without publication. Standalone descriptor and relay tests must refuse without native execution; they do not establish isolation from the supplied marker. Preserve source-bound logs, binaries, artifact hashes and terminal outcomes under the increment's evidence directory.

The supported loader/CuDNN ABI is explicit and narrow. Equivalent owner processes can still alter backing files. The research helper does not prove aggregate production admission or independently bounded parent filesystem operations. Durable launch, acceptance, retirement and restart integration remain required before GPU scheduling can be enabled. No model, device or release qualification is implied.

Rollback: keep GPU scheduling disabled and stop dispatching the private CUDA roles. Retain existing durable-intent reader barriers and evidence; no schema downgrade or destructive project rollback is part of this change.
