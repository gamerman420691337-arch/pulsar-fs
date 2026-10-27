# CUDA adapter feasibility and observed dependencies

This is local research evidence for GB-02/B3, not a production GPU adapter or release qualification. Pulsar's current production admission still supports only CPU. Its CUDA Cargo feature does not change that behavior. The experiment used an authored graph and no private model/media, training inputs, sterile references or Handy device.

## Observed execution

The identified machine has an NVIDIA RTX 4090, driver 615.71.09 and 24564 MiB reported device memory. Installed packages report `onnxruntime-opt-cuda 1.29.0-3`, `cuda 13.4.2-1` and `cudnn 9.27.0.42-1.1`. These version strings are observations; the retained file hashes identify the tested bytes more precisely.

The [reproducible harness](prototypes/cuda-runtime-probe.mjs) compiles the [C++ probe](prototypes/cuda-runtime-probe.cpp) against the installed ORT headers/runtime. It uses the existing authored temporal ONNX fixture, four constant inputs and an independent expected output `[0.25, 0.375, 0.5, 0.75]`. CUDA registration sets `session.disable_cpu_ep_fallback=1`; profiling must then show exactly twelve computation events, all on the requested provider. Registration and successful output alone are insufficient.

The installed CUDA provider initially refuses to load with unresolved symbol `cudnnGetConvolutionBackwardDataAlgorithm_v7`. It has no cuDNN `DT_NEEDED` entry. Explicit preloading of the already-installed local cuDNN library resolves this experiment; the underlying packaging/build cause was not established or repaired. CUDA/cuDNN versions and dependencies must match the selected runtime, as described by [ORT's CUDA requirements](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html#requirements).

Final observed cases:

| Case | Observation |
| --- | --- |
| Host CPU | Four outputs pass the oracle; twelve computation events report CPU. |
| Host CUDA, default environment | Refuses before execution; unresolved cuDNN symbol. |
| Host CUDA, explicit local cuDNN preload | Four outputs pass; twelve computation events report CUDA, without CPU computation. |
| Namespace without GPU device nodes | Specific CUDA error 100; no generated output. |
| Namespace with GPU nodes but no required sysfs | Specific CUDA error 304; no generated output. |
| Namespace with explicit nodes and restricted read-only sysfs | Four outputs pass; twelve computation events report CUDA. |

The successful experimental namespace exposes only `/dev/nvidiactl`, `/dev/nvidia0`, `/dev/nvidia-uvm`; read-only NVIDIA/NVIDIA-UVM module `initstate` files; CPU/node topology directories and the memory block-size file. It mounts the selected cuDNN directory and installed CUDA library directory separately. Capability dropping remains explicit and `/usr/bin/nvidia-modprobe` is replaced with `/usr/bin/false` inside the namespace. This avoids enabling a module-loading helper. No host libraries, permissions, routing or services were changed.

An earlier namespace attempt with device nodes and no sysfs failed. Its syscall trace identified missing sysfs accesses; a broad read-only sysfs experiment passed, and the final narrower mount set also passed. A two-file sysfs attempt failed with CUDA error 999. These are observed experiments, not a claim that the final set is minimal or portable to other driver/device versions.

## Resource and dependency limits

The CUDA arena is limited to 64 MiB and TF32 is disabled in the probe. This does **not** impose a whole-worker VRAM quota: ORT explicitly says total device usage can exceed its arena limit. [CUDA arena limit documentation](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html#gpu_mem_limit)

Runtime/provider/selected cuDNN files and selected build tools/headers are hashed before and after the final campaign. The probe emits loaded mapped paths after inference; the harness hashes those files. Those observations cover this graph and moment, not every future lazy-loaded dependency, all compiler inputs or a complete runtime lock. No model quality, sustained load, cold/warm throughput, peak whole-worker VRAM, other GPU/backend or cross-platform result follows.

## Production work still required

1. Extend the declared execution contract to bind selected GPU/device identity, native dependency facts, explicit resource reservations and actual provider evidence. Preserve strict CPU behavior and refuse unsupported adapters.
2. Add engine-owned device/resource admission and finite worker confinement using approved dependencies and explicit device nodes. Do not bind the full host device tree or import arbitrary supplier native code into engine/client processes.
3. Route actual detector/temporal sessions through the selected adapter and validate node assignments, including unsupported-node refusal and new-attempt-only fallback. Keep catalog/pack choice, source/profile identities and immutable job dependencies intact.
4. Measure bounded termination, memory, contention, model equivalence and required throughput on identified configurations. Qualify model/provider profiles separately; this tiny graph does not supply those gates.

The [evidence packet](../../docs/evidence/roadmap-2026-10-01/cuda-runtime-probe/validation.json) retains final and failed diagnostics. At that probe checkpoint, production source was byte-identical to the preceding checkpoint, so the full Rust suite was not rerun or relabeled as new GPU evidence. The subsequent [strict trace validator](../../docs/PULSAR_STRICT_INFERENCE_TRACES.md) runs new Rust gates, validates these retained bytes, checks a fresh CUDA trace and identifies the separate virtual-address-space requirement; it still does not enable production CUDA admission.
