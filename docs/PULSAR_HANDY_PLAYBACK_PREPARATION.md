# Prepare native Handy playback inputs

Pulsar can retain a nonplaying preparation from an exact acknowledged setup:

```sh
pulsar handy-prepare-playback PROJECT --revision PROJECT_REVISION \
  --setup-request-id ORIGINAL_SETUP_REQUEST --record-id SETUP_RECORD_SHA256 \
  --engine-epoch ENGINE_EPOCH --device-session DEVICE_SESSION \
  --hosting-request-id HOSTING_REQUEST \
  --qualification-id DOSSIER_ID --qualification-revision REVIEW_REVISION \
  --request-id PREPARATION_REQUEST --allow-online-inspection
```

The original setup actor needs current project Read/Export grants, the private Handy owner
capability, a live captured connection and current accepted qualification. Pulsar performs
[fresh stream correlation](PULSAR_HANDY_PROGRAM_CORRELATION.md) and checks every reference
again before retaining native state. The transport uses the captured credential snapshot;
it does not reload environment values during construction.

The result reports `state: prepared`, an exact `preparation_id`, and the execution artifact
bound to the downloaded hosted program. The runtime remains disarmed with no controller.
`armed` and `playback_enabled` are false. No setup, play, stop or mode-change command is sent.
This is not a way to move a toy yet.

## Original-request recovery and retirement

```sh
pulsar handy-playback-status PROJECT --preparation-request-id PREPARATION_REQUEST

pulsar handy-retire-playback PROJECT \
  --preparation-request-id PREPARATION_REQUEST \
  --preparation-id PREPARATION_SHA256 --request-id RETIREMENT_REQUEST
```

Status and retirement need current owner capability and project Read access. They do not
need the old revision, credentials, source files or a network connection when the native
entry is already absent. Use a separate retirement request ID. A lost retirement reply is
recovered by repeating that same complete selection; it cannot retire a later replacement.

The Engine writes the preparation receipt before installing its private native state. A
failed receipt write installs nothing. Exact preparation retry returns the existing state
without new remote reads. Missing native state, including after restart, returns `retired`;
receipts cannot rebuild it. Create a new preparation with a new request only after reviewing
the current selection and acquiring any new setup/session required by restart.

Only one preparation is retained per logical device, with at most 64 live preparations.
Explicit retirement frees this local
slot; it does not clear setup-route reservations or retract a remote setup attempt.
Project revision/grant changes, connection close, registration changes, qualification changes
and forgotten hosting retire dependent preparations before the changing operation replies.
Status also rechecks retained references and current qualification without remote I/O.

`prepared` describes retained host state, not continuing observation of the remote device.
Future arming and handoff must independently recheck current device and authority facts;
status does not refresh them over the network.

## Stopping uncertainty and controller lifetime

Any recorded play/stop effect for the selected logical device sets `physical_stop_pending`
in its new runtime. This is a complete existence query, not a bounded history page. A new
setup or a matching stopped-stream response does not clear that uncertainty. Absence of a
matching record is only local history absence; it does not prove physical stopping or map
different registrations to the same physical hardware.

A persistent paired session is authentication, not controller liveness. Ordinary CLI and
desktop requests close their IPC connections after each call. Preparation therefore grants
no controller identity and never enables headless operation. The separate
[controller lease workflow](PULSAR_HANDY_CONTROLLER_LEASES.md) now provides local controller
ownership and arming after fresh checks. Server/device/browser timing, dispatch/stop integration
and actual qualification remain necessary before physical playback is available.

## Verification scope

The 2026-10-22 campaign passes 325 Protocol, 116 Handy Engine, 24 Handy Client (one existing
desktop case ignored), 14 setup-journal and 51 playback regression tests. Both strict lint
configurations, app/worker build, formatting and architecture/source checks pass.

Engine fixtures cover construction, exact replay, all six immediate retirement boundaries,
receipt-write failure, restart and retained effect uncertainty. A reproduced replay defect
was fixed by preserving the current authenticated request during internal revalidation.
Native CLI checks use authored history to exercise absent-native-state recovery and an actual
dropped committed retirement reply. Exact retry and restart preserve the retirement receipt,
and setup/effect journals remain empty. All owned Engines are joined. Initial test/lint/harness
failures remain in the evidence packet with the corrections. No real device or physical
acceptance is part of this campaign. See
[ADR B5-037](../research/phase-b/ADR-B5-037-boot-local-nonplaying-preparation.md) and
[checkpoint](evidence/roadmap-2026-10-22/handy-playback-preparation/checkpoint.json).
