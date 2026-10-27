# Inspect the stream associated with a Handy setup

The CLI can recheck a stopped stream against one acknowledged setup:

```sh
pulsar handy-inspect-program PROJECT --revision PROJECT_REVISION \
  --setup-request-id ORIGINAL_SETUP_REQUEST --record-id RECORD_SHA256 \
  --engine-epoch ENGINE_EPOCH --device-session DEVICE_SESSION \
  --hosting-request-id HOSTING_REQUEST \
  --qualification-id DOSSIER_ID --qualification-revision REVIEW_REVISION \
  --request-id INSPECTION_REQUEST --allow-online-inspection
```

Use a separate inspection request ID. The original setup actor must hold the private Handy
owner capability and current project Read/Export grants. The selected setup must have retained
its validated response and mode generation, belong to the current Engine/connection, and
match the selected current revision, registration, qualification and hosted program.
Legacy history missing response or mode data cannot acquire those facts through inspection.

The Engine uses retained credentials and GET requests only. It checks device identity,
operative settings, mode generation and stopped HSSP state, rechecks the exact downloaded
script, then samples device state again. It checks authority and all references after I/O.
A changed stream, mode, registration, qualification, connection or project refuses the result.
No setup, upload, mode change, play or stop is sent. Repeating the inspection performs new
reads; it does not return a cached mutation receipt.

## Read the result

`inspection` retains the selected program and current device metadata.
`acknowledgment` is the original setup response. `first_buffer` and `second_buffer` are the
complete bounded HSP fields observed around the hosted-byte check. `expected_samples` gives
the complete verified script's sample count and first/last timestamps.

Buffer population and endpoints may change during loading. HSSP uses HSP streaming, including
adding points during playback, so a script can exceed the device's buffer capacity. This
inspection permits that case. [Ohdoki Firmware 4 guide](https://intercom.help/ohdoki/en/articles/9457599-what-s-new-in-firmware-4)

The API describes buffer metadata rather than a device-content hash, and does not declare
whether the absolute tail index starts at zero or one. Pulsar therefore does not treat whole-
script residency or `tail = count - 1` as required proof. Script-relative buffer/index
interpretation remains a qualification obligation. [REST v3 schema](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)

The result establishes only the sampled stream association under the checked references.
`content_verified` and `playback_enabled` remain false. It cannot prove exact intermediate
positions, exclusivity against another controller, present physical state or stopping.
A saved report cannot construct native playback authority or survive a restart as permission.
The separate [playback preparation command](PULSAR_HANDY_PLAYBACK_PREPARATION.md) performs its
own fresh checks before retaining disarmed native inputs. It never consumes a saved client
correlation report as permission.

## Storage and validation

New setup intent records retain their preflight mode generation. Existing record bytes and
identities remain unchanged when that optional field is absent. Readers predating the field
must refuse it; no default mode generation is invented.

The 2026-10-22 campaign passes 323 Protocol, 111 Handy Engine, 23 Handy Client (one existing
desktop case ignored), 14 setup-journal and 51 playback regression tests. Both strict lint
configurations, app/worker build, formatting and architecture/source checks pass. The final
qualification recheck has its own memory reservation after the earlier inspection budget ends.

Software fixtures cover streamed buffers smaller than the script, fresh repeated observations,
changed selection/state and in-flight revocation. A loopback HTTP fixture decodes the complete
buffer from the same state GET and withholds numeric credential echoes in the additional fields.
The actual CLI refuses missing consent, retired Engine boots and legacy missing mode history
without creating mutation receipts. The preceding reader rejects the new mode field with
unchanged database/WAL bytes; SQLite shared-memory sidecars can change. Native history is
explicitly authored, all owned Engines are joined and no real device is contacted. See
[ADR B5-036](../research/phase-b/ADR-B5-036-fresh-stream-correlation.md) and
[checkpoint](evidence/roadmap-2026-10-21/handy-program-correlation/checkpoint.json).
