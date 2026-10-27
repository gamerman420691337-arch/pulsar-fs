# ADR B3-022: independently supervise native image capture

Status: implemented capture primitive; durable GPU admission remains disabled

## Problem

The existing capture helper copies and rehashes up to eight GiB synchronously.
Calling it from the authority control path would make cancellation and live
control depend on slow source reads and snapshot I/O.

## Decision

Move byte capture into an explicit private process role using the shared protocol,
aggregate resource pool and Linux confinement. Select nofollow regular source
descriptors, bind their identities into the request and verify mounted input
coordinates inside the child. Permit writes only to a newly owned capture stage.
Bind completion to the exact request and attempt. Adopt only that owned stage's
bounded image metadata; full hashing remains supervised in the child.

Drive all pipes without blocking, bound diagnostics/replies and apply one parent
deadline. Retain resources and staging during unconfirmed retirement. Release
CPU capacity after observed process-tree retirement; retain storage until image
close. Use explicit private directory permissions rather than temporary-directory
defaults. Keep ordinary inference workers' output tmpfs and file limits unchanged.

## Evidence and limits

See [the capture contract](../../docs/PULSAR_SUPERVISED_NATIVE_CAPTURE.md) and
its source-bound validation packet. A declared 74-library capture and independent
readback succeed without executing the captured libraries. Cancellation/deadline
cases confirm eventual cleanup and resource release. Initial compile errors,
the umask-dependent directory refusal and a harness filename collision remain
historical failure records, not evidence for the final subject.

Parent filesystem setup/metadata/cleanup is not asynchronously interruptible.
Kernel uninterruptible I/O can retain custody beyond a caller's deadline. Restart
fencing, durable attempt admission, whole-worker dependency closure, mounted
image integrity and actual GPU sessions remain separate implementation work.
Model/media, physical Handy, platform and human/release qualification remain open.
