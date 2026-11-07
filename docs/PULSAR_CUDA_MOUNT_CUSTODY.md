# Request-bound captured-library and CUDA device mounts

The shared Linux confinement builder now has a CUDA mount assembly path.
`PinnedNativeMounts` borrows a captured image and holds an `O_PATH` descriptor for
its library directory and every declared alias. It checks alias inode, size and
read-only mode against captured content before pinning. Namespace setup overlays
each alias from its held descriptor. Replacing a directory entry after pinning
therefore does not select the replacement file during mount setup.

`CudaMountCommand::inspection` checks the validated CUDA request against the
captured directory, exact declared aliases and held GPU UUID/PCI/minor identity.
It creates an attempt cgroup and uses the shared confinement builder to mount
the image and the existing held CUDA device nodes before executable arguments.
Its descriptor limit is 1,024; ordinary CPU and capture limits remain 128 and
512. Fallible mount setup now propagates through the existing CPU/live/capture
callers. The assembly borrows the captured image and device handles; callers
still own authorization, aggregate reservations and complete child retirement.

The dedicated `cuda-mount-check` role accepts a bounded private witness. It
checks exact request/alias correspondence, mounted device and file inodes, every
alias hash and post-read metadata. It performs no device IO, native library
loading or inference. Its witness is not owner approval, durable intent,
accepted result evidence or a public project format. CPU worker requests and
portable format 9 are unchanged; this increment makes no database-schema change.

## Evidence

The [source-bound packet](evidence/roadmap-2026-10-01/cuda-mount-custody/validation.json)
retains 901 Rust passes (seven qualification cases ignored), 440 CUDA-feature
engine passes (one ignored), all 98 configured JavaScript cases without skips,
223 static structure checks and 37 unchanged executable/metadata identities.

The actual current executable runs inside an attempt cgroup and the shared
bubblewrap namespace. Inspection verifies 83 aliases from the declared 74-library
image and three held NVIDIA nodes. A baseline and an alias-replacement case pass;
altered alias inode, altered device inode and changed captured backing bytes
refuse. Each inspected child exits and its cgroup is observed empty before mount
custody is dropped. The owned capture is removed and all 74 original library
files are independently rehashed against their specification. No production
job, CUDA inference or physical actuator is exercised by these cases.

The research helper captures bytes in its parent process. It does not establish
production aggregate RAM/storage admission or bounded parent capture. Production
must use the existing supervised capture path under current reservations.
The initial compile failure and preliminary native run have historical logs;
their failed/preliminary source subjects were not archived separately.

## Remaining launch integration

Use this mount assembly with fresh owner/resource admission, supervised image
capture, the schema-15 attempt ledger and the actual CUDA worker bootstrap.
The later [confined CUDA worker bootstrap](PULSAR_CUDA_WORKER_BOOTSTRAP.md)
adds explicit descriptor erasure before request intake and native execution,
held bindings for two fixed kernel module-state files, and a verified captured
loader transition. The historical mount packet predates these changes and
records only its own checker and separate bubblewrap descriptor observation;
it does not qualify the later bootstrap or another bubblewrap version.

Read-only guest mounts do not freeze mutable backing inodes. Hash checks detect
observed changes; they do not prevent later owner-process writes or undo native
effects. Complete execution custody, independent deadlines, cancellation,
confirmed process-tree retirement, restart recovery and durable accepted
receipt/trace membership still need integrated production evidence.

GPU job admission remains disabled. This mount checkpoint does not qualify
models, media accuracy, physical VRAM limits, platforms, performance, Handy,
human usability or a release.
