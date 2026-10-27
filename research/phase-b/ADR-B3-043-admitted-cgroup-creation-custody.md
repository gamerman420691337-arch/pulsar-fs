# ADR B3-043: own the reservation before admitted cgroup creation

Status: implemented creator primitive; terminal current-source positive gates pass; retained legacy counterexample fails; production integration remains open.

The format-4 journal describes ownership but cannot itself keep a live reservation and lease through fallible creation. Introduce an opaque admitted creator that installs custody before validation, acknowledges Intent before mkdir through its held recorded parent, captures identity before fallible handoffs, binds before configuration and acknowledges registration before transferring into worker custody.

The creator never exposes a group handle. Its successful transfer installs process-tree custody without IO. Failure moves the full attempted journal state, held parent/child identities, resource claim and fence lease into retrying cleanup. Unknown capture remains ambiguous, replacements refuse, retirement precedes deletion, actual absence precedes clear and successful clear precedes releasing claims. Cleanup cannot manufacture deletion authority from a scoped name.

Native cases exercise private journal write failures, blocked deletion, ambiguous capture, an unlinked-root replacement and a real owned child after transfer. They are not production admission or full engine crash evidence. Production activation must commit schema 20 before creating resources; staged-byte ownership must also transfer on failure. Native capture, legacy recovery, reboot/relocation, portable correspondence and independent qualification remain required.

See [implementation and evidence](../../docs/PULSAR_CGROUP_CREATION_CUSTODY.md).

[ADR B3-044](ADR-B3-044-production-admitted-cgroup-creation.md) continues with production admission and staged-file transfer on failure and success.
