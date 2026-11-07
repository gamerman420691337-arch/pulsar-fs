# Confined CUDA worker bootstrap

The private Linux `cuda-worker` role executes an explicitly declared CUDA generation request inside the shared confinement builder. Ordinary worker profile admission remains CPU-only and the engine still refuses production GPU scheduling. This increment closes the confined native worker and actual artifact pipe seam; it does not authorize a job or accept a project result.

## Execution contract

The parent retains the captured library directory, every alias descriptor, the three pinned NVIDIA device descriptors and two fixed sysfs module-state descriptors. Only `/sys/module/nvidia/initstate` and `/sys/module/nvidia_uvm/initstate` are added as read-only module metadata. Host preparation requires actual sysfs files, root ownership and `live` state. The child checks their mounted identities, sysfs type and state; it does not infer host ownership from remapped user-namespace IDs.

Before reading control input, the fresh child requires its confinement marker and erases all descriptors above stderr with `close_range`. The marker is a private routing precondition, not proof of isolation or owner authorization. The received witness must bind the exact canonical request, declared image aliases and mounted device identities. The child verifies the complete mounted image under one monotonic deadline before choosing the captured loader.

The initial executable restarts through the verified `ld-linux-x86-64.so.2`, with the captured directory as its explicit library search path and the host loader cache inhibited. A bounded memfd carries the exact witness and original absolute monotonic deadline across this exec. It is sealed against writes, growth, shrinkage and seal changes. The second role requires that sealed descriptor, reads and closes it, erases inherited descriptors again, and rechecks namespace identities and bytes. Exec does not grant a fresh time budget or leave the cgroup, namespace or artifact pipe.

The currently supported runtime requires declared `libcudnn.so.9` in global symbol scope: its CUDA provider references CuDNN symbols without a corresponding ELF `NEEDED` entry. After second-stage verification, the worker loads only that captured alias with `RTLD_NOW | RTLD_GLOBAL` and retains its handle through execution and evidence emission. This is an explicit supported ABI requirement; there is no host-library or CPU fallback.

The verified bootstrap permits the shared tensor adapter to construct the strict CUDA session. The ordinary public worker entry point still refuses the same CUDA profile. Dependencies and mounted custody are checked again before successful output. The existing framed result and bulk artifact stream carry the program, general receipt, dedicated CUDA receipt and original node trace to the actual parent reader. Failed execution cannot emit successful CUDA evidence.

## Reproduction and verification

`crates/pulsar-engine/examples/cuda-worker-check.rs` captures the declared image and generates from an authored two-frame RGB input and ONNX model. It launches the actual confined role, receives artifacts through the production parent reader, independently reads them back, verifies the expected motion value, observes child exit and an empty cgroup, and explicitly closes the owned capture. Changed request identity, alias inode and device inode refuse without artifact publication. The JavaScript test separately exercises descriptor erasure, default-build refusal, and missing/unsealed second-stage relay refusal.

The image requires approximately 1.94 GB of snapshot storage. The recorded commands use the private `/home` cache as `TMPDIR`; a preliminary `/tmp` run refused insufficient storage before inference. This does not permit deleting shared temporary files or bypassing storage admission.

The [source-bound packet](evidence/roadmap-2026-10-01/cuda-worker-bootstrap/validation.json) records 903 default Rust passes, 442 CUDA-feature engine passes, all 101 configured JavaScript cases without skips, 224 structural checks and 40 unchanged executable/metadata identities. Its 174 production and 124 verification paths have archive and current-source readback. Seven default qualification tests remain ignored. Four generated artifacts and 135 retained artifact records have hash readback.

The retained pre-module source and binary subject reproduces CUDA initialization status 304. A minimal driver probe isolates the necessary two module-state files; device remount and capability changes alone do not fix it. Later loader-search and CuDNN-symbol failures have retained logs but no separate immutable source subjects. They must not be treated as independently archived regression subjects. The first full campaign passed 100 cases and failed one incorrect canary-length assertion; its separate source subject and log are retained. Correcting the JavaScript fixture produced the final 101-case pass without a production or Rust-verification change.

## Remaining obligations

The research helper captures in its parent and owns its own attempt supervision. It does not establish production aggregate admission, durable launch, accepted-evidence membership, independent parent filesystem deadlines, cancellation/restart recovery, whole-device VRAM accounting or owner authorization. A sealed relay ensures immutable transport, not authenticity against a process with equivalent owner authority. Read-only mounts do not freeze mutable backing inodes or undo native effects.

CUDA preview remains unsupported. The authored result does not qualify learned model quality, reference alignment, real-media speed, physical Handy operation, six-axis playback, other OS/GPU vendors, human acceptance or release readiness. These remain separate roadmap gates.
