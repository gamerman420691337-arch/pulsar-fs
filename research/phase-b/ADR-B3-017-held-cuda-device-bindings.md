# ADR B3-017: held CUDA device bindings

Status: implemented kernel/node consistency prerequisite; production CUDA remains unsupported.

## Decision

Match explicit UUID/PCI/minor expectations against root-owned procfs GPU metadata and character-device registrations. Hold exactly the control, selected GPU and UVM nodes with no-follow `O_PATH` descriptors. Keep parent descriptors close-on-exec; clear that flag only in the forked launch child. Do not load modules or modify host nodes.

Use inherited descriptor sources and verify mounted inode, filesystem and device numbers inside confinement before CUDA library preload/session creation. Binding implementations can resolve descriptor links; do not call an argument containing a descriptor a complete mount-identity proof. The experimental native gate refuses altered tables and nodes before executing its fixed CUDA runtime command.

## Authority and compatibility

The helper does not grant driver access, establish logical CUDA ordinal mapping, freeze physical GPU identity through resets or implement VRAM isolation. The integrating owner broker must authorize the expanded driver surface, own descriptor/child lifetime and resource admission, and persist appropriate device/dependency/attempt reader and recovery facts. The worker must verify its actual vendor SDK identity.

No engine command, durable profile/database schema, default CPU pool or provider admission changes. The Linux research example requires a device-expectation file; this is not an engine wire change or application qualification.

## Evidence and rollback

[Interface and limits](../../docs/PULSAR_CUDA_DEVICE_BINDINGS.md) and [receipt](../../docs/evidence/roadmap-2026-10-01/cuda-device-bindings/validation.json) retain actual inherited-descriptor mounts, the pre-CUDA guest gate, authored numerical/trace checks and six refusal cases. The failed parent-fd route and transient gate compile failure remain separate. No private media or physical playback is used.

The helper has no enabled production launch caller. Withdrawing it changes no durable CUDA state; production enablement still requires the remaining owner/device/SDK/resource/recovery work.
