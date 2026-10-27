# Handy connection sessions

Pulsar's Engine owns explicit, boot-local Handy connection observations. They reserve
one logical registration and one connection-key route while retaining the exact native
credential snapshot used for inspection. They do not arm a device, set its mode, upload
a script or grant playback authority. The public `physical_playback` capability remains
false.

## CLI workflow

Use the current executable (the development build is
`/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar`). Run:

```sh
pulsar handy-registrations
pulsar handy-sessions
```

The first command returns saved registration revisions and binding hashes. The second
returns the current Engine epoch and its session observations without network I/O.
Open an explicitly selected binding with captured values:

```sh
pulsar handy-open-session DEVICE \
  --engine-epoch ENGINE_EPOCH \
  --expected-registration-revision REVISION \
  --binding-sha256 BINDING_SHA256 \
  --request-id ORIGINAL_OPEN_ID \
  --allow-online-inspection
```

The two credential variables named by that registration must exist in the Engine's
environment. Keep their values in the encrypted secret store and launch the Engine
through `secrets run -- ...`. An already running Engine does not inherit credentials
from a later CLI invocation. Missing values refuse before contacting the device.

The result has an Engine-generated `device_session` identifier and a state:

- `opening`: a bounded inspection is still in progress.
- `observed`: connectivity and identity were observed using the retained credentials.
- `retired`: that original opening no longer owns a connection observation.

`observed` is historical observation, not a continuous liveness claim, exclusive control
of the physical device, timing qualification or proof of a stopped device. Session status
never polls or reconnects automatically. Retire the local observation explicitly:

```sh
pulsar handy-close-session \
  --engine-epoch ENGINE_EPOCH \
  --device-session DEVICE_SESSION \
  --request-id ORIGINAL_CLOSE_ID
```

Closing sends no remote command and is not a physical stop operation. These observation
sessions cannot own an active dispatcher yet. Physical controller integration must add
owned dispatcher retirement and preserve all durable stop obligations before enabling
that capability.

## Replay, concurrency and lifetime

An opening receipt commits before inspection. Retrying exactly the original request
returns its current boot-local state without repeating network I/O or reloading keys.
The CLI preserves the supplied revision/hash pins and does not replace them from a newer
catalog. A changed original request conflicts. Opening failures after reservation retire
the original identity; retry reports `retired`. A new attempt needs an explicit new ID.

Closing and registration changes invalidate the observation. An in-progress inspection
keeps its route reserved until its bounded GET returns, even if closed or invalidated.
Its late reply cannot restore the session. A committed registration change retires its
observation under the DB-to-registry lock order; a rolled-back change leaves it intact.
An exact close retry returns its historical retirement receipt. Other owner-authorized
clients can deliberately close a selected host session; ordinary project grants do not
confer this authority.

Restart restores no credential values, connection observations, consent or authority.
Every open/close request carries its captured Engine epoch, so a delayed request from an
older boot refuses. Changing that epoch under the original ID also conflicts. Opening
receipts contain only metadata. Preceding readers cannot decode these response variants
and have no matching session commands; they cannot turn the receipts into activation.

Credential values and route comparisons remain native and in memory; no secret hashes
or values are stored in receipts or returned to clients. Aliases that resolve to the
same connection-key value are excluded, even under different environment names or API
keys. Different keys are not assumed to identify different physical devices. Physical
identity and external-controller exclusivity still require separate verified evidence.
At most 64 sessions are retained. Closed/completed-failed observations release their
slot; durable original IDs continue to prevent reopening.

The existing conditional HSSP transport and inspection broker now share one credential
validation representation. The session registry retains its snapshot, but the public
Engine does not yet attach that snapshot to a qualified playback dispatcher. Qualification
intake, loaded-script setup and hosting consent, controller lifecycle, fresh identity
and clock verification, hardware stop/order behavior and rendered desktop session controls
remain open. See [dispatch](PULSAR_HANDY_DISPATCH.md),
[registration](PULSAR_HANDY_REGISTRATIONS.md), and
[checkpoint](evidence/roadmap-2026-10-16/handy-sessions/checkpoint.json).


## Scoped verification

The complete Protocol suite passes 304 tests. Engine Handy checks pass 45, including
eight new connection-lifecycle cases; all 51 coordinator/dispatcher tests pass. Client
Handy checks pass 15 with one existing isolated-desktop case ignored. The native CLI
campaign verifies public capability discovery, explicit consent, missing-credential
refusal before reservation, unknown-session close refusal, fresh Engine epochs and stale
open/close refusal after restart. It creates one inert registration and no projects,
jobs, candidates or exports. Its two Engine processes are joined and worker custody is
empty. Networking is isolated and no real device is contacted.

Successful opening and its races use injected Engine observations; the existing native
HTTP inspector and dispatcher tests cover their separate transport paths. This is not
an end-to-end successful connection to a Handy. Strict default/all-feature workspace
lint, App/Worker builds, format and architecture checks pass. The final discovery-only
correction reruns affected Engine/lint/build checks and retains the unchanged Protocol,
Client and coordinator results from the preceding successful source snapshot.

The initial stale credential-field test compilation error and Unix-socket scratch-path
failure are retained and corrected. No test assertion or production deadline was relaxed.
Full ordinary-storage Engine/Client suites, desktop session controls/rendered acceptance,
physical synchronization/stopping and all model/media/platform/human release gates remain
open. These checks do not close the full roadmap.
