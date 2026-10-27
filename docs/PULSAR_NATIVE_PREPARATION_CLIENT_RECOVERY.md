# Native preparation client recovery

CLI and desktop save inert Start metadata before sending the request. A transport error or lost reply leaves that identity available after reopening. Recovery resolves the original request under current authentication and project revision. It does not replay Start automatically, renew a deadline, change frozen options or establish launch authority.

The journal stores project, original revision, request ID, engine epoch and duration options. It contains no bearer credential, API key, executable bytes or media paths. A credential hash and endpoint hash bind the namespace to the original client identity. Storage uses the shared private, owned, single-link, bounded-file implementation with nonblocking locking, atomic replacement and fsync. Unsafe, corrupt, mismatched or full storage refuses Start before dispatch. Capacity is 64 entries; exact duplicate intent is permitted and conflicting reuse is refused. Current status, cancellation and release do not depend on the local journal.

The Capabilities panel loads saved identities on opening a project or inspecting limits. Recover is explicit; replies must match the session, project, original request, epoch, revision and options. The historical intent is retained even when resolving under a newer current revision. Forget saved identity only removes local metadata; it does not cancel or release engine work. Engine reports remain the authority for operation state and cleanup.

```sh
pulsar native-preparation saved PROJECT
pulsar native-preparation resolve PROJECT --revision CURRENT_REVISION --original-request-id ORIGINAL_ID
pulsar native-preparation dismiss PROJECT --original-request-id ORIGINAL_ID
```

This increment covers client metadata recovery. It does not qualify OS death or power loss, graphical desktop usability, CUDA reliability, annotated model accuracy, live Stash, Handy 2 Pro firmware/secrets/physical stop and synchronization, other platforms or release. Full roadmap remains PARTIAL.


Verification: seven focused final-source client recovery tests pass; strict client Clippy across all targets, application checks across all features/targets, scoped formatting, 350 architecture checks and 36 source guards pass. The broader concurrent client suite remains unqualified: its 180-second command times out after reporting failures in four existing recovery concurrency tests. Initial compilation errors were fixed; those failed attempts remain in the packet. No full-suite success is claimed. The [checkpoint](evidence/roadmap-2026-10-10/native-preparation-client-recovery/checkpoint.json) preserves logs and source identities.
