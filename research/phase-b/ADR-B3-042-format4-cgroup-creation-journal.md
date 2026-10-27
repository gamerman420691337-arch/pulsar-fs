# ADR B3-042: journal creation separately from registered worker ownership

Status: primitive and current-source positive gates pass; legacy counterexample fails; admitted creator and production integration pending.

A directory that exists after mkdir but before registration is neither a registered worker nor an absence obligation. Format 4 therefore retains a mandatory creation roster, keyed by a scope-qualified canonical UUID leaf, with Intent, Bound and Retired phases. The roster binds the delegated parent identity and Linux boot ID. Canonical older ledgers omit the new field. Schema 20/minimum 4 remains the database reader requirement.

Absent Intent may clear; existing Intent is ambiguous and refuses without deletion. Bound requires an exact existing empty identity. Retired may recover exact emptiness or absence. Retirement precedes deletion, and confirmed absence precedes clear. Atomic transfer removes Bound creation ownership and adds the same identity to the active worker roster. Ordinary format-4 registration must not bypass this transfer.

Preflight covers all ownership families before cleanup. A bounded scope-specific namespace observation rejects unrecorded resources without claiming ownership of other scopes. Changed parent or boot identity refuses; automatic relocation/reboot recovery is not silently inferred. Existing registered legacy groups remain explicit identities during upgrade.

These facts do not create resources or own a reservation by themselves. The admitted creator must install custody before fallible effects, use the held recorded parent for mkdir/open, bind before configuration, retain reservation/lease on lost acknowledgements, and transfer to worker custody before spawn. Production activation must publish format 4, commit schema 20, and only then expose the creator. Full engine cuts, hostile same-UID races and power-loss/platform qualification remain independent obligations.

See the [native evidence and implementation limits](../../docs/PULSAR_CGROUP_CREATION_JOURNAL.md).

[ADR B3-043](ADR-B3-043-admitted-cgroup-creation-custody.md) continues with the admitted reservation/lease owner. Production activation and integration remain open.
