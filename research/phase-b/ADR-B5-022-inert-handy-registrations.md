# ADR B5-022 — Persist credential references without restoring device authority

The HSSP worker needs a stable configured target, while current physical identity,
qualification, script loading and live session ownership remain separate facts.
Store bounded owner-controlled credential references as inert registrations. Save
their changes and ordinary request receipts in one Engine transaction so lost replies
can be recovered through the exact original identity without replacing newer state.

Use the existing host-local Handy owner capability plus session authentication for
all catalog/configuration/inspection operations. Reads do not initialize metadata.
Registration does not resolve credentials or authorize networking. Inspection requires
separate consent and an exact current revision/fingerprint, rechecked with authentication
after I/O. Shared clients capture the selected binding and validate response correspondence.

Self-versioned metadata tables preserve an entry incarnation, checked revision and
configuration fingerprint. They have no qualified/playable state and do not change
existing project schema meaning or portable formats. The fingerprint changes on
recreation as well as updates; neither it nor a historical acknowledgment restores
authority. Unknown schema/layout/data refuses instead of being repaired automatically.

This additive compatibility is intentionally limited to inert configuration. Old
readers do not understand these references. Physical activation needs a separate
reviewed reader/state barrier, verified identity and credential/session binding,
qualification intake and current Engine authority. Disabling lookup does not stop
a physical device. See [contract and evidence](../../docs/PULSAR_HANDY_REGISTRATIONS.md).
