# Runtime installation stage recovery

2026-10-11. Recognized-stage component checks PASS; roadmap PARTIAL.

The local host commands are:

- runtime-stage-inspect ABSOLUTE_STAGE: report a live lock or an integrity-bound inactive stage inventory.
- runtime-stage-discard ABSOLUTE_STAGE --expected-manifest-sha256 HASH --approve-discard: explicitly remove a recognized inactive stage after fresh checks.

Neither command starts Engine, performs inference, loads a runtime, selects a model or accesses credentials. The commands can operate without HOME or PULSAR_STATE_DIR. Approval authorizes this stage deletion only. Inspection is read-only apart from acquiring a temporary advisory lock. Active stages report state:locked with no claimed manifest identity; recognized inactive stages report state:discardable, manifest hash and partial file/byte counts. Those counts are inventory, not payload integrity or usable-package qualification.

## Receipt and deletion boundary

The installer writes .pulsar-runtime-stage.json before payload copying, locks it exclusively and synchronizes it. It contains schema1, the original candidate manifest JSON and its exact hash. Receipt capture is bounded to512 KiB and the original manifest to64 KiB. The private receipt remains as local metadata in a published installation; it is additional to the fifteen payload records and original candidate manifest. It grants no model or inference approval. Published destination basenames are rejected by the stage-discard path.

The stage must have the generated .pulsar-runtime-stage- basename convention, private owned0700 directory and private owned single-link0600 receipt, under an owned parent that other users cannot write. Manifest schema and its finite known membership are checked before inventory. Partial managed payloads may be shorter than their declared length; inspection/discard does not assert they are valid executable/model bytes.

Discard requires the explicit flag and exact manifest hash, then a nonblocking exclusive lock. The entire bounded inventory is checked before deletion begins. Unknown entries, symlinks, hardlinks, changed inode/device bindings, unexpected modes/ownership/devices and oversized files refuse. Only recognized leaf files and directories are removed with held descriptors; no generic recursive delete is used. The receipt is removed last and directory removals are synchronized. If a filesystem error causes partial removal, retry must revalidate remaining metadata. Final parent-sync uncertainty is reported explicitly.

Locks coordinate Pulsar operations, not arbitrary software with the user's full OS privileges. Unmarked legacy stages, incomplete/corrupt receipts and unexpected contents are preserved for manual inspection. Automatic collection, exhaustive concurrent external mutation, filesystem faults/power loss, update/rollback and final distribution qualification remain open.

## Verified behavior

Four recovery unit cases cover an active lock, explicit approval/stale identity, complete preflight preservation of an unknown owner file, safe managed-subtree removal with an external symlink target preserved, and corrupt/missing receipt or published-basename refusal. Complete selected default libraries pass: Engine775/zero failures/59 ignored (3.88s), clients221/zero failures/four ignored (9.67s). These runs use private2 GiB tmpfs and disabled network. Ignored native/full-workspace/all-feature library gates are separate.

The actual CLI/installer campaign observes an active writer and refuses discard, sends SIGKILL and reaps that exact child, then inspects11,026,432 partial bytes in one file. Stale hash refuses. An injected unknown file refuses before deleting/changing the known partial payload, whose independent hash remains unchanged. After the fixture removes its own injected file, explicit discard removes the recognized stage. Retry succeeds under umask0777 with all fifteen payload identities and original manifest intact. Existing destination/owner file remain unchanged. This native test passes once without skips in2.793s; it is authored package/recovery wiring, not model or hardware qualification.

Ordinary supplier assembly and actual first-party installation also succeed. The retained updated candidate is /home/brianklam/Projects/Funscripts/assets/pulsar-runtime-recovery-2026-10-11/candidate. Current application/worker bytes include both recovery commands. All payload identities and private local receipt correspondence are independently rechecked. A published candidate basename refuses inspection/discard; this is not an uninstall command.

[ADR B3-103](../research/phase-b/ADR-B3-103-runtime-installation-stage-recovery.md) and [checkpoint](evidence/roadmap-2026-10-11/runtime-stage-recovery/checkpoint.json) retain exact source/binaries, native observations, gates and cleanup. Source/model rights, useful generation defaults, Core/Complete extraction/update/rollback and data relocation, broader storage/OS/vendor/media/live/Handy 2 Pro and human/release acceptance remain open.
