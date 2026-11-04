# Owner-controlled Handy registrations

Pulsar can save, list and disable local Handy credential references through its
authenticated Engine. A registration contains a logical device ID, display label,
two environment-variable names and a lookup-disabled flag. Saving reads no Handy
credential values and sends no network request. It establishes no physical identity,
qualification, live session, loaded script or playback authority.

Use the rebuilt development executable and Engine. The [workstation quickstart](PULSAR_APP_QUICKSTART.md) gives the executable path; an older running Engine retains its old command set.

```sh
pulsar handy-register handy-main --label 'Handy 2 Pro' --api-key-env HANDY_API_KEY --connection-key-env HANDY_CONNECTION_KEY --request-id handy-register-1
pulsar handy-registrations
```

The environment variables may be provisioned later through the encrypted secret
store. Never supply key values in these fields. When credentials are available in
the Engine's environment, explicit read-only inspection is:

```sh
pulsar handy-inspect-registered handy-main --allow-online-inspection
```

The client captures the current catalog entry and sends its revision and binding
fingerprint. Automation can additionally pin both with
`--expected-registration-revision N --binding-sha256 HASH`. The Engine rechecks the
binding and authentication after the existing consent-bound GET inspection. A
disabled entry, stale revision/fingerprint, concurrent update or revoked session
cannot return an observation as if it still belonged to the selected registration.
Inspection remains ephemeral and does not qualify a device.

For updates, use `handy-register` with the current
`--expected-registration-revision` and a new `--request-id`. `--disabled` saves an
entry with lookup disabled. To disable an existing entry while preserving its
settings and history:

```sh
pulsar handy-disable handy-main --expected-registration-revision 1 --request-id handy-disable-1
```

Disable blocks registered lookup; it sends no device stop command. Saving an entry
without `--disabled` explicitly enables lookup again at a new revision.

## Recovery and storage

Each save/disable and its original request receipt commit in one Engine database
transaction. Retain the request ID and exact arguments if a reply is lost. Exact
replay returns the historical acknowledgment without applying the change again.
It does not describe current configuration: query `handy-registrations` separately.
Reusing an accepted ID with changed arguments refuses. After a definite revision
conflict, inspect current configuration before submitting a distinct new update.

Every change advances a checked revision and changes a fingerprint that includes
the retained entry incarnation and all settings. An erased/recreated row receives
a new incarnation, so its fingerprint cannot masquerade as the old binding even
when its logical ID and revision match. Configuration fingerprints are references,
not approvals. Different registrations or environment names may still refer to the
same physical device; live ownership must resolve actual identity separately.

The bounded catalog holds at most 64 entries. Settings, IDs, revisions and stored
rows are validated; no key values, owner capabilities or inspection consent are
stored in these tables. The feature uses self-versioned inert metadata tables in
the existing private Engine database. Current readers validate their layout and
contents before Engine initialization/recovery mutations. Unknown versions,
incomplete layouts and malformed rows refuse without repair. Main project schema
semantics and portable project formats are unchanged. Older readers do not support
these commands or enforce these references; this compatibility choice is valid only
while registration remains non-authoritative. Activating physical sessions requires
its own reviewed state/reader contract.

The CLI and future desktop controls share one request helper with response
correspondence checks. [Desktop controls](PULSAR_DESKTOP_HANDY_REGISTRATIONS.md) now expose this workflow. Rendered GUI acceptance, live device/session ownership,
qualification/evidence intake, verified script setup, controller/transport wiring,
physical Handy/six-axis, model/media, platform and human/release acceptance remain
open. Public playback remains disabled; the full roadmap is PARTIAL.

See [decision](../research/phase-b/ADR-B5-022-inert-handy-registrations.md),
[transport worker](PULSAR_HANDY_DISPATCH.md), and
[verification](evidence/roadmap-2026-10-16/handy-registration/checkpoint.json).


## Scoped verification

Engine cases exercise owner/session checks before storage, no project or physical
side effects, atomic config/receipt rollback, historical replay through restart,
conflicting IDs, revision/fingerprint refusal, concurrent rebind and authentication
loss during inspection, recreated entries, and corrupt metadata rejection before
startup writes. Client cases verify exact retry identities and response correspondence.
Protocol cases reject malformed or qualification-bearing records and project-scoped
registration requests. A separate boundary regression preserves the preexisting
catalog/selection exhaustion checks and permits their read-only terminal revision.

The isolated native CLI campaign uses private RAM storage, disabled networking and
absent Handy credentials. A Unix-socket proxy drops the actual committed registration
reply; repeating the original command recovers its receipt. Updates, disabled lookup,
historical replay and current state survive Engine restart. Stale/current-with-missing-
credentials inspections refuse; unknown metadata versions refuse startup with unchanged
database/WAL bytes. Final fixture state contains one registration and zero projects,
jobs, candidates or exports. Owned Engine/proxy processes join with empty worker custody.
No real device was contacted or qualified.

Initial failures remain retained: a Rust temporary-borrow lifetime error, nonprivate
mock owner-directory permissions, lint ordering/slice findings, an unintended broad
revision-comparison edit caught during diff review and restored, a too-small 128 MiB
native RAM filesystem, and a ledger audit that assumed an optional creation roster.
The native harness now uses the preceding campaigns' 1 GiB limit and validates ledger
fields according to their version. Production storage and authorization guards were
not relaxed. Final counts, command outcomes and source/binary identities are in the
checkpoint. Full ordinary-storage suites, rendered desktop behavior and independent
platform/model/device/human acceptance remain open.
