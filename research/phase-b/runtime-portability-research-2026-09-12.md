# Good phase runtime portability research

Research date: 2026-09-12. Status: primary-source research and proposed implementation sequence. This document does not qualify a runtime, model, GPU, sandbox, release artifact, or product-quality claim.

## Scope and source-audit baseline

Preserve Windows x86-64, Linux x86-64, Apple Silicon macOS, and Intel macOS as launch targets. Preserve NVIDIA, AMD, Apple, and Intel GPU coverage as explicit qualification obligations. Keep the existing four Rust libraries, the executable's engine/client/worker roles, and engine authorization of effects.

The coordinating source-audit agents reported the following against HEAD `4da4c08`; these are supplied code observations, not independently repeated runtime tests:

- `crates/pulsar-protocol/src/worker.rs:92`: `WorkerTools` carries an optional ONNX Runtime path.
- `crates/pulsar-engine/src/worker/detector.rs:27`: detector creation builds a CPU session; preprocessing uses channel-major RGB NCHW.
- `crates/pulsar-engine/src/launch.rs:10`: confinement availability is Linux-only and depends on `/usr/bin/bwrap` and cgroup availability.
- `.github/workflows/release.yml:48`: release packaging archives the executable without bundled inference assets.

These observations establish implementation gaps, not a reason to drop a target. This research accessed public documentation only. It performed no device access, GPU run, model download, private-file inspection, paid job, or release qualification.

## Verified provider capabilities

ONNX Runtime partitions supported nodes/subgraphs across ordered execution providers; availability of an EP does not establish that the entire model executes on that accelerator. Its documentation also warns that combined-provider builds need their dependent libraries present. [ORT execution-provider architecture](https://onnxruntime.ai/docs/execution-providers/)

| Runtime/provider | Relevant targets | Dependency and capability facts | Consequence for Pulsar |
| --- | --- | --- | --- |
| ORT CPU | Windows/Linux x86-64; macOS x86-64 and arm64 builds | Default EP; native runtime and selected model operators still need packaging. ORT supports platform-specific native builds. | Required local baseline and recovery path; pin native ABI, build features, model format, OS floor, and CPU instruction baseline. |
| ORT CUDA | NVIDIA on Windows/Linux | CUDA-enabled NVIDIA GPU; compatible CUDA and cuDNN libraries. cuDNN major versions are not interchangeable. | Bundle permitted runtime libraries, retain an installed compatible driver requirement, and report the actual executing provider. |
| ORT TensorRT | NVIDIA on Windows/Linux | Adds TensorRT; ORT recommends registering CUDA for unsupported TensorRT nodes. Cached engines depend on model, precision, profiles, versions, and GPU. | Optional optimization candidate; avoid treating a shipped engine cache as a universal binary model. |
| ORT DirectML | Windows x86-64, compatible NVIDIA/AMD/Intel GPU | Requires DirectX 12. Documented minimum is Windows 10 1903; operator coverage is narrower than arbitrary ONNX coverage. | Broad Windows GPU candidate, subject to an actual model/operator and adapter campaign. |
| ORT CoreML | Apple Silicon and Intel macOS builds with CoreML enabled | EP API floor macOS 10.15; MLProgram requires macOS 12. Compute-unit options include CPU, CPU+GPU, CPU+ANE, or all compatible units. | Prefer a native Apple path; ANE presence and GPU execution must be observed separately. |
| ncnn Vulkan alternative | Windows/Linux x86-64; macOS through MoltenVK | C++ inference runtime with C API, CPU and Vulkan backends; PyTorch/ONNX conversion tools produce ncnn models. | Provides a candidate Linux AMD/Intel path, but requires a separately qualified conversion and integration. |

Sources: [ORT native builds](https://onnxruntime.ai/docs/build/inferencing.html), [CUDA EP](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html), [TensorRT EP](https://onnxruntime.ai/docs/execution-providers/TensorRT-ExecutionProvider.html), [DirectML EP](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html), [CoreML EP](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html), [ncnn upstream](https://github.com/Tencent/ncnn).

DirectML sessions require sequential execution, disabled memory patterns, and serialized `Run` calls per session. The documented EP currently supports through opset 20 with stated operator exceptions. Provider configuration must therefore be backend-specific. [DirectML requirements and configuration](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)

The ncnn maintainers mark NVIDIA, AMD, and Intel on Windows/Linux as known working, and Apple on macOS as known working. Intel and AMD on macOS are explicitly unconfirmed in that matrix. The build guide includes a macOS x86-64/arm64 MoltenVK configuration. These upstream reports are useful candidate evidence, not Pulsar's qualification. [ncnn Vulkan FAQ](https://github.com/Tencent/ncnn/wiki/FAQ-ncnn-vulkan), [ncnn build guide](https://github.com/Tencent/ncnn/wiki/how-to-build)

MoltenVK implements a subset of Vulkan over Apple's Metal, rather than introducing an independent Apple GPU driver. Every selected operation and precision mode must work within that subset. [MoltenVK upstream](https://github.com/KhronosGroup/MoltenVK)

### Current documentation drift and version selection

The retrieved ORT CUDA page describes CUDA 13 as the default from ORT 1.27; the install page still describes CUDA 12 as the default. The TensorRT EP compatibility table also stops earlier than the CUDA page. Do not derive a shipping dependency set from unversioned "latest" instructions. Choose an exact ORT release/build and verify its actual linked libraries and matching provider documentation. [CUDA compatibility table](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html), [ORT installation](https://onnxruntime.ai/docs/install/), [TensorRT compatibility table](https://onnxruntime.ai/docs/execution-providers/TensorRT-ExecutionProvider.html)

The CoreML API floor is not the deployment floor of a prebuilt runtime. ORT's current build page says the default macOS deployment target is 13.3. An older Intel Mac support claim requires an appropriately built runtime and clean-host proof. [ORT macOS build requirements](https://onnxruntime.ai/docs/build/inferencing.html)

DirectML is in sustained engineering; ORT recommends considering Windows ML for new Windows work. Windows ML supports a self-contained native runtime, and vendor EPs can be bundled separately instead of acquired at runtime. Thus Windows ML is a real packaging alternative, not automatically incompatible with offline use. Its exact bundled runtime, providers, license set, and Rust ABI integration remain a selection gate. [ORT installation status](https://onnxruntime.ai/docs/install/), [Windows ML deployment](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/distributing-your-app)

## Redistribution and dependency boundaries

These are verified source terms and remaining artifact checks, not a distribution approval.

| Component | Verified redistribution basis | Required artifact-level work |
| --- | --- | --- |
| ONNX Runtime | MIT; preserve copyright and permission notice. | Include actual release notices and transitive dependency inventory. [ORT license](https://github.com/microsoft/onnxruntime/blob/main/LICENSE) |
| CUDA runtime | CUDA SDK license grants conditional distribution of designated object-code components. Attachment A lists runtime and math-library files, including versioned variants. SDK use is scoped to NVIDIA GPU systems. | Select only required permitted files; retain applicable terms; verify acquired version's notices and transitive closure. The toolkit table is not evidence that a modern macOS CUDA runtime exists. [CUDA EULA and Attachment A](https://docs.nvidia.com/cuda/eula/index.html) |
| cuDNN | SDK supplement designates runtime `.so` and `.dll` files as distributable; underlying requirements include material application functionality and application-only access. | Preserve exact SDK agreement and notices; verify every shipped runtime file against that version. [cuDNN agreement and supplement](https://docs.nvidia.com/deeplearning/cudnn/backend/latest/reference/eula.html) |
| TensorRT | SDK supplement designates runtime `.so` and `.dll` files as distributable, subject to the SDK agreement and NVIDIA-system scope. | Check the selected version's allowed files and dependencies; open-source wrapper/plugin licenses do not replace the runtime agreement. [TensorRT agreement and supplement](https://docs.nvidia.com/deeplearning/tensorrt/latest/reference/sla.html) |
| DirectML | Microsoft's 1.15.4 package license permits specified application/service/game distribution for Windows and Xbox; it is not ORT's MIT license. | Preserve license and third-party notices; review exact package's data/update terms and demonstrate offline behavior. License wording does not establish that a telemetry event occurred. [DirectML 1.15.4 license](https://www.nuget.org/packages/Microsoft.AI.DirectML/1.15.4/License) |
| Microsoft C/C++ runtime | ORT's Windows package requires the VC++ runtime; Microsoft limits redistribution to designated redistributable files and applicable product terms. | Bundle a permitted app-local closure or use an appropriately licensed static build; an installer prerequisite would fail the proposed first-launch contract. [ORT requirements](https://onnxruntime.ai/docs/install/), [Microsoft redistribution guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170) |
| ncnn | BSD 3-Clause, with separately identified components under other licenses. | Preserve actual compiled components' notices; qualify the selected C API/static-library integration and converters used at build time. [ncnn license](https://github.com/Tencent/ncnn/blob/master/LICENSE.txt) |
| MoltenVK | Apache 2.0. | Inventory its selected build and dependencies and preserve their notices; link to the host's Apple frameworks. [MoltenVK licensing](https://github.com/KhronosGroup/MoltenVK#licensing) |
| Model weights, conversion outputs, codecs, fonts, shaders | Runtime licenses do not establish rights in these separate assets. | Release remains blocked until exact assets, provenance, modification/export permissions, notices, sizes, and hashes are selected and recorded. This research selects no model. |

Proposal: distinguish bundled application dependencies from an explicitly declared supported OS, kernel facilities, graphics stack, and GPU driver. Requiring users to install Python, a package manager, CUDA toolkit, runtime SDK, model, or sandbox helper would fail the intended portable launch. No claim of independence from an OS or installed GPU driver follows from embedding libraries.

## Confinement and resource limits

### Linux

Bubblewrap constructs a policy using Linux user namespaces and command-line bindings; it is not a complete security policy by itself. Its current upstream has removed the historical setuid fallback. Bundling the helper therefore does not enable user namespaces on a host that prohibits them. [Bubblewrap security and user namespaces](https://github.com/containers/bubblewrap)

cgroup v2 requires delegated access and enabled controllers. `memory.max` is the hard-limit mechanism for accounted memory, with documented temporary overshoot; `memory.high` throttles and is not that limit. A self-contained executable cannot grant itself missing delegation. [Kernel cgroup v2 delegation and memory controller](https://docs.kernel.org/admin-guide/cgroup-v2.html)

Proposal: package a pinned sandbox helper and its allowed library closure, probe actual namespace creation and delegated controller writes before worker acceptance, then launch directly into the controlled group. Bind only approved input, immutable assets, bounded output/scratch, IPC, and the exact GPU interfaces needed by the selected provider. Keep network absent. Missing isolation must produce an explicit unavailable state rather than an unconfined worker.

### Windows

Job Objects manage process groups and termination. Extended job limits constrain committed virtual memory per process and per job; these are not interchangeable with Linux RSS accounting or a GPU-memory quota. AppContainer provides resource isolation and explicit network capabilities. [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), [Job memory limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information), [AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)

Proposal: start the worker suspended, apply an appropriately restricted AppContainer/token and narrowly granted resource access, attach a job with bounded memory/process limits and kill-on-close, then resume. Preserve engine-only effect authorization over constrained IPC. Test inherited handles, child escape, job-assignment failure, adapter access, denied network, and engine death using that exact packaging. A Job Object alone does not establish filesystem/network confinement.

### macOS, both architectures

Apple's sandbox guidance prefers XPC for privilege separation. A child using sandbox inheritance needs the specified sandbox/inherit entitlement combination and inherits static rights, not automatically every file right acquired after launch. Entitlements attach to signed code identities. [Apple sandbox inheritance](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html), [Apple code-signing procedure](https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/Procedures/Procedures.html)

Proposal and unresolved gate: design the mapping from the executable's existing engine/client/worker roles to signed sandbox identities before implementation. Selecting `--worker` cannot by itself supply a different code-signature entitlement set. A packaged helper/XPC identity might be needed; whether that satisfies the single-executable packaging contract requires an explicit design resolution, not a silent architectural relaxation.

Apple's archived `setrlimit` manual describes `RLIMIT_RSS` as influencing reclamation when memory is tight; it does not establish a cgroup-equivalent aggregate physical-memory kill boundary. A current supported mechanism and live adversarial evidence for the required macOS bound remain unresolved. [Apple setrlimit manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setrlimit.2.html)

### GPU budgets

CUDA's `gpu_mem_limit` caps the EP arena, and total device allocation may exceed it. TensorRT's workspace limit is likewise a workspace setting. Neither documents a whole-worker VRAM quota. [CUDA arena limit](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html#gpu_mem_limit), [TensorRT workspace option](https://onnxruntime.ai/docs/execution-providers/TensorRT-ExecutionProvider.html#trt_max_workspace_size)

Proposal: enforce finite input dimensions, queue depth, sessions, precision, scratch and output sizes, allocator budgets, and a worker deadline; retain an external supervisor and cancellation/kill path. Record host-memory and device-memory evidence separately. Do not label monitoring or cooperative allocation limits a hard total GPU bound, especially on shared/unified memory.

## Proposed offline first-launch contract

These are implementation choices, not existing behavior or upstream guarantees:

1. Produce one target-specific download containing the executable and every required model/runtime/helper asset. Embed a trusted manifest with target, format/ABI version, byte count, digest, purpose, and applicable notices. Preserve the four-library architecture by placing asset and platform launch machinery inside the existing engine boundary.
2. Extract to an owner-controlled, versioned content-addressed location. Use an exclusive staging directory and launch lock; prohibit traversal, links, alternate data streams and special files; cap individual and total unpacked bytes. Validate bytes against the embedded trust anchor before making the generation visible by atomic rename.
3. Reuse only a complete validated generation. Handle interruption, concurrent launch, corrupt cache, low disk, read-only install location, spaces and non-ASCII paths. Never repair by downloading. Report required free space before substantial extraction.
4. Resolve the full native dependency closure from trusted packaged locations plus explicitly approved OS/driver libraries. Loading the root Windows DLL by absolute path is insufficient: dependencies can still be searched by module name. Configure the loader's dependency search before loading any provider. [Microsoft DLL dependency search](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order)
5. Preserve signatures and entitlements on executable assets; qualify the actual macOS distribution and loading arrangement. Do not modify signed native libraries after signing or assume that extraction alone preserves all deployment expectations. [Apple distribution signing](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/)
6. Bring up the GUI and engine, validate confinement, then run a small bundled inference self-check with networking denied. Report model hash, runtime build, provider selected, actual graph assignment/fallback, target, and failure reason. Provider probing stays inside a bounded worker, since native provider loading is executable code.

## Smallest implementation sequence

1. Specify target OS floors, CPU instruction baseline, RAM/VRAM definitions, maximum first-launch size/time, and the required GPU/model output tolerances. Record release-asset identities and redistribute only the selected, permitted closure.
2. Implement bundled CPU runtime/model extraction and trusted loading within the existing engine. Replace the optional external runtime prerequisite for the packaged default path. Keep explicit internal runtime identity for evidence and development.
3. Implement the three platform launch adapters. Resolve macOS signed identity and memory-bound questions early; replacing a Linux-only error with ordinary process spawning would not complete this step.
4. Prove CPU first launch on clean Windows/Linux x86-64, Apple Silicon macOS, and Intel macOS hosts, with no SDKs/toolchains/model cache and networking denied. CPU functionality must survive an absent or failed GPU provider without misreporting acceleration.
5. Add model-specific GPU execution: a pinned DirectML or self-contained Windows ML choice for Windows; CoreML for macOS; ncnn Vulkan as the leading Linux AMD/Intel candidate. Evaluate NVIDIA through that Vulkan path or a pinned CUDA closure. Keep TensorRT as an optimization candidate unless measured requirements justify its packaging and compilation cost.
6. Qualify every required vendor/platform combination, conversion parity, actual operator placement, cancellation, exhaustion, corrupted assets, driver failure, and restart. Publish only the completed rows as verified; retain outstanding required rows as launch blockers.

A GPU fallback can keep a task usable, but cannot satisfy an unpassed GPU launch obligation. No target or vendor is removed by this ordering.

## Required unresolved decisions and qualification rows

| Item | Evidence needed to close |
| --- | --- |
| Exact runtime and asset selection | Pinned release/source identity, ABI/features, dependency inventory, model redistribution record, notices, bytes and hashes. |
| Windows GPU packaging | DirectML versus self-contained Windows ML decision; exact model opset/provider coverage; clean-host closure and three-vendor evidence. |
| Linux GPU packaging | ncnn model conversion/parity, Vulkan loader/driver access, and NVIDIA/AMD/Intel runs under the real worker policy; optional CUDA closure separately. |
| Apple Silicon GPU | CoreML or Vulkan selected path, actual GPU/ANE assignment, signed distribution, denied-network launch and unified-memory evidence. |
| Intel macOS GPU | A selected compatible Intel/AMD adapter and supported macOS/runtime combination with measured execution. Upstream ncnn leaves these GPU rows unconfirmed. |
| macOS confinement and resource ceiling | Supported signed identity design consistent with role boundaries; isolation and exhaustion campaign proving the promised limit. |
| Linux restricted hosts | Observed user-namespace and controller availability, plus explicit supported-host floor; no installation or privilege escalation hidden in launch. |
| Offline first launch | Fresh-cache artifact-driven run per target; no network, package-manager, SDK or model-fetch dependency; signed asset behavior and corruption/recovery evidence. |
| Product quality | Held-out quality/performance acceptance for the selected model and each precision/conversion path. Provider documentation and toy inference do not establish Good-phase completion. |

No code, tests, devices, models, or release gates were exercised for this research note. The next runtime work should produce artifact-specific evidence against these rows.

