# Guarded Handy setup

The CLI can explicitly send one setup request for a current, verified hosted script:

```sh
pulsar handy-setup PROJECT --revision PROJECT_REVISION \
  --engine-epoch ENGINE_EPOCH --device-session DEVICE_SESSION \
  --hosting-request-id HOSTING_REQUEST \
  --qualification-id DOSSIER_ID --qualification-revision REVIEW_REVISION \
  --request-id ORIGINAL_SETUP_REQUEST --allow-online-setup
```

Use the current development executable if `pulsar` is not on PATH. This command loads no
authority from a saved client report. The Engine independently requires current Read/Export
grants, the private Handy owner capability, accepted physical qualification for the running
control build/configuration, a retained connection and verified hosting reference. The actual
physical evidence must cover setup behavior as well as the declared HSSP profile; software
fixtures are not qualifying evidence. No real device was used to implement or test this path.

Preflight rechecks hosted bytes, qualification, sampled script limits, device identity and
operative settings. The device must already report HSSP mode, stopped or not initialized,
normal rate and no looping. These are remote observations, not physical stopping proof.
The command changes no device mode, uploads no new script, and sends no play or stop command.

## Handoff and outcome

The Engine reserves the original actor/request fingerprint, commits the setup record and a
cross-store record link, then consumes its single-use journal claim. It checks authority and
references again before handoff. HTTP runs outside project, session-registry and journal
locks, under a shared 30-second operational budget. The fixed vendor setup endpoint receives
only the retained native URL and `notify: false`, using the captured credential values.
Redirects, proxies and automatic application retries are disabled. URLs and credentials do
not enter the journal or public response.

A successful response must be bounded and structurally valid, report a fresh stream ID and
non-playing, non-looping normal-rate state. This is recorded as `acknowledged`, not proof that
the complete intended script is loaded or physically executable. The sanitized response is
now [retained atomically with that acknowledgment](PULSAR_HANDY_SETUP_OBSERVATIONS.md),
providing historical stream data for [fresh correlation](PULSAR_HANDY_PROGRAM_CORRELATION.md).
New setup records also retain their preflight mode generation; older missing modes remain
unknown. Errors, lost replies, stale
state and deadline failures remain uncertain. A failed final metadata write leaves `claimed`
history. Neither case permits another automatic setup attempt.

The returned status always has `loaded: false` and `qualified: false`. Playback remains
unavailable. Attempted records keep their logical-device and credential-route reservations
until separately implemented reconciliation can justify release. This includes acknowledged
setup: repeated loading of another script is not yet an end-to-end workflow.

## Recovery

Retain the exact original request ID and selection. Once a journal record exists, repeating that same setup request returns
history, without new device/hosting reads or another PUT. Changed request selection refuses.
For read-only recovery, including after owner-capability changes, use:

```sh
pulsar handy-setup-status PROJECT --setup-request-id ORIGINAL_SETUP_REQUEST
```

A global request reservation can exist before a journal row if preparation failed. Its absence
from setup history is not permission to invent a replacement effect. A retained prepared row
is never automatically claimed on retry. The shared CLI provides
[exact unclaimed cancellation](PULSAR_HANDY_SETUP_CANCELLATION.md), including recovery after
a lost cancellation reply. Attempted-effect reconciliation remains unfinished.

The project database links every record before it can be claimed. Startup checks those links
before writable storage initialization. Missing linked records or a missing journal refuse
startup without recreating history. This protects ordinary partial-store loss; it is not proof
of disk power-loss behavior or protection against arbitrary coordinated rollback of all stores.

## Contract and verification scope

The owning [REST v3 schema](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml) defines
the setup operation and HSP response fields. The current parser intentionally requires complete,
bounded numeric observations and a different stream ID. Actual firmware compatibility and
physical effects remain qualification work.

Positive Engine dispatch tests use explicit simulated qualification and injected observations.
Separate loopback HTTP tests exercise the actual single PUT, headers/body, malformed/error
responses, dropped replies, deadline expiry and numeric credential-echo withholding. The native
CLI campaign checks consent, missing-session refusal, replay of seeded historical work and
missing-journal refusal. It does not perform a successful real-media/device setup. See
[ADR B5-033](../research/phase-b/ADR-B5-033-explicit-qualified-setup-handoff.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-send/checkpoint.json).
