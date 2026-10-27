# ADR B3-026: request-bound descriptor custody for CUDA mounts

Status: implemented mount assembly and confined inspection; GPU launch disabled

## Problem

A captured pathname can be replaced between metadata checking and namespace
setup. The shared confinement builder also needs to place CUDA mounts before
the executable arguments and propagate failures without silently omitting a
mount. Existing request declarations do not establish actual mounted identity.

## Decision

Hold the captured directory and every alias through `O_PATH` descriptors. Mount
the directory read-only and overlay all aliases from their held descriptors.
Bind the plan to the validated request and the existing pinned CUDA devices.
Use the shared confinement builder and a private request-bound witness to
inspect actual namespace inode/hash correspondence without loading native code.
Keep production GPU admission disabled until admission, supervision, worker
bootstrap, accepted results and retirement are integrated.

## Evidence and limits

See [the mount contract](../../docs/PULSAR_CUDA_MOUNT_CUSTODY.md) and its packet.
Actual namespace cases cover baseline, host alias replacement, altered library
and device identities, and corrupted captured bytes. Unit checks cover invalid
tables, symlinks, backing-mode changes and request correspondence. Existing
CPU/live/capture entry points pass their configured regressions.

Borrowing capture/device custody is not aggregate admission or retirement
proof. Read-only mounts do not freeze backing contents. Descriptor erasure
before native loading, supervised production capture and complete CUDA worker
lifecycle remain required. Model/media/platform/performance/physical/human and
release qualification remain independent.
