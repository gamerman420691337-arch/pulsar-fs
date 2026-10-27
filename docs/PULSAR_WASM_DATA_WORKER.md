# Data-only WASM worker

Status: verified local software increment. The later [Linux supervision increment](PULSAR_WASM_WORKER_SUPERVISION.md) adds attempt-bound wire and an engine-side helper; GB-12 remains incomplete.

The optional `wasm` feature uses pinned Wasmtime 49.0.2 with `std`, `runtime` and `cranelift`; default features, WASI and component hosting are absent. `pulsar wasm-worker` now reads one bounded version/attempt envelope from stdin, writes one corresponding response, and exits. Bare declarations refuse; the original packet below tested the earlier worker bytes. The Engine does not dispatch this role. An example worker executable exposes the same implementation for process tests. Third-party guest code is executed in that worker role, not by the engine request handler or GUI.

## Buffer ABI

`pulsar.buffer.v1` is a core binary-module ABI for Processor and Editor roles. Editor is a manifest role here; it grants no editing authority. Requests carry captured module bytes, their manifest, input bytes and a separate `guest_memory_bytes` bound. The module SHA-256 and exact length must agree with its identity before compilation. No precompiled native artifact is deserialized. The result carries module/input/output hashes and consumed fuel; its output is untrusted data requiring future semantic and engine admission.

A guest exports:

- `memory`: one ordinary wasm32 linear memory.
- `pulsar_alloc(i32 length) -> i32 pointer`: the guest owns allocation; the host checks its returned range before writing input.
- `pulsar_process(i32 pointer, i32 length) -> i64`: upper 32 bits give output offset; lower 32 bits give length. The host validates the range and budget before copying it.

No imports are admitted, including WASI, filesystem, network, device, engine and allocation host calls. Every invocation creates a fresh Engine/Store/instance. The current implementation explicitly reserves 16 MiB of guest address space with zero growth reservation and a 64 KiB guard for finite native address-space admission. The guest cannot access project memory or manufacture a grant. A raw result is not a candidate receipt, motion edit, native exception approval or device qualification.

## Implemented limits and remaining supervision

Transport is bounded to 4 MiB before JSON parsing; modules to 512 KiB; input and output to 64 KiB each. Unknown fields refuse. Guest memory is separately bounded from 64 KiB through 16 MiB, with one memory, one instance, one table and at most 4096 table elements. Memory-growth failures trap. Guest stack is bounded to 256 KiB; memory64 and multiple memories are disabled. Fuel is capped at one million, including instantiation/start functions and allocation. A joined timer arms epoch interruption before instantiation, and elapsed-deadline checks refuse late results after compilation and before/after execution/copying.

The manifest retains the whole-worker resource declaration: 64–512 MiB memory, two CPU slots (guest/native work and timer), positive output budget at most 64 KiB, and a positive wall budget at most five seconds. These declarations are checked, **not yet enforced as OS limits**. Wasmtime's Store limits cover guest memory; they do not bound all native compiler/runtime allocations. Guest interruption does not preempt native compilation or blocked stdin/stdout. The later [Linux supervisor](PULSAR_WASM_WORKER_SUPERVISION.md) supplies these process/pipe/cancellation/retirement mechanisms behind an internal helper. The standalone harness remains unenforced at OS level, and authenticated engine dispatch with immutable native-build/input custody and semantic project admission is still required.

Upstream distinguishes guest interruption from blocking host-call deadlines. This ABI admits no guest host calls; it does not claim a host effect broker has been implemented. [Wasmtime configuration](https://docs.rs/wasmtime/49.0.2/wasmtime/struct.Config.html), [Store resource limits](https://docs.rs/wasmtime/49.0.2/wasmtime/struct.StoreLimitsBuilder.html), [pinned upstream release](https://github.com/bytecodealliance/wasmtime/releases/tag/v49.0.2).

## Verification

Historical evidence for the initial worker source/wire: [sealed local packet](evidence/roadmap-2026-10-09/wasm-data-worker/checkpoint.json).

- Actual Wasmtime kernel: **7 passed, 0 failed, 0 ignored**; identity and fresh stores, real input transformation, epoch interruption, fuel exhaustion in start/processing loops, rejected imports/signatures, memory growth and bad guest ranges.
- Full protocol test executable: **102 passed, 0 failed, 0 ignored**, including the three new admission/transport cases.
- Actual example worker process: **9 passed**. One positive transformation and eight refusals verify stdin/stdout behavior, exact hashes, no output on refusal, changed bytes/length, invented authority, Driver role, host import, loop, output range and oversized transport. This runs the same `run_worker` entry point used by composition; the root command itself was compile-checked, not exercised as a shipped binary.
- Root application `cargo check --locked --offline` passes with `wasm` and with default features. The test build reports 27 Engine warnings; ordinary library/composition builds report 37. These are not warning-clean gates. No CUDA/TensorRT or cross-platform build was run.
- Final architecture: **301 passed**; lexical source guards: **36 passed**. Only the three new Rust files were format-checked. Static checks are not runtime confinement qualification.
- Final source hashes match the successful build receipts. The scoped inventory covers workspace crate Rust sources/manifests plus root Cargo files and composition: 299 records; six existing files changed, three new files, 290 unchanged. The original lockfile package versions are retained; 55 package/version records were added.

The initial 180-second check timeout, missing offline test dependency, stopped pre-patch build, five compile errors before the explicit Wasmtime-to-anyhow error bridge, and initial wrong example-binary path remain separately recorded. None is relabelled as a passing gate. The failed process harness never launched a worker; correcting its path did not change production guards or fixture bytes.

These are local synthetic software checks. Native compile/process/pipe resource enforcement, semantic project admission, natural-media usefulness, platform security, human acceptance and release readiness are not established.

## Roadmap work still required

GB-12 still needs authenticated engine/client dispatch, captured immutable input/artifact lifetime, aggregate scheduling and enforced supervisor boundaries, semantic motion/result admission, protected-region preservation and undoable edit receipts, revocation and late-result fencing, plugin discovery/versioning, broker-call deadlines for future effectful ABIs, native Runtime/Driver roles and exact-build exception/per-invocation/standing approvals. Cross-platform malicious-extension, recovery and release qualification remain open. GB-11 local assistant execution and memory handoff are separate unfinished work. No model training, annotations, natural-media quality or physical-device claim is made.
