# Read-only inspection before Handy setup

`handy-inspect-setup` checks a retained hosting location and connection observation against
the exact current project revision. It performs GET requests only. It does not upload a
script, change device mode, call HSSP setup, play or stop, or construct physical stopping
or qualification evidence.

Use the current development executable
`/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar` in place of `pulsar` if needed:

```sh
pulsar handy-inspect-setup PROJECT --revision REVISION \
  --engine-epoch ENGINE_EPOCH --device-session DEVICE_SESSION \
  --hosting-request-id HOSTING_ID --request-id INSPECTION_ID \
  --allow-online-inspection
```

Select an observed [connection session](PULSAR_HANDY_CONNECTION_SESSIONS.md) and a verified,
locally retained [hosting result](PULSAR_HANDY_HOSTING.md) from the same authenticated client
session. Both must belong to the current Engine boot. The owner capability, Read/Export
grants, registration binding, current revision and master hash are checked before I/O and
again before returning. Missing consent, stale bindings, retired locations/connections and
old epochs refuse. A historical hosting receipt alone cannot restore a lost native location.

## What is checked

The Engine captures the existing native credential snapshot and hosting reference without
rereading credential environment variables or accepting a caller-supplied URL. It then:

1. Reads connectivity, identity, device mode, slider configuration and stroke limits using
   the retained device credentials.
   If mode is HSSP, it also reads the current remote HSSP state.
2. Downloads the retained hosted script without device credentials and requires its exact
   byte count and SHA-256 to match the previously verified download.
3. Reads the same device observations again. Identity, mode generation, operative settings and the selected
   HSSP fields (play state, stream ID, current time, rate, looping and starvation policy)
   must agree between the samples; identity must also match the captured connection.
4. Rechecks current authorization and both native references after I/O.

All reads share one 30-second operational deadline. This is not a device timing
qualification or a calibrated source/device clock. The reported monotonic observation span
and host wall-clock timestamp describe this inspection only. Two agreeing samples are not
an atomic remote transaction or proof that another controller cannot act between/after them.

Mode generation is required locally even though the vendor schema makes it optional.
Missing, fractional, oversized or unsupported values refuse; no default generation is
invented. HSSP state is omitted outside HSSP mode. A reported `stopped` state is an API
observation, never proof of physical stopping. The existing remote stream may belong to a
different script: this read-only operation has not loaded the inspected hosted script.

The returned report binds project/revision/master, script/download identities, registration,
connection and hosting request. It explicitly keeps `setup_performed` and `qualified` false.
It contains no native URL or credential values, creates no durable request receipt, and is
not an admission token. A fresh explicit query repeats reads; restoring an old report cannot
trigger I/O. A future setup path must acquire current qualified authority and fresh references
itself, rather than accept this report as a client assertion.

If hosting is forgotten, the connection is closed, the registration changes, grants are
revoked or the project changes during inspection, the late result refuses. Already-issued
GETs cannot be retracted, but their result cannot revive retired ownership or authority.
A changed or expired download is not silently re-hosted. Requiring exact hosted bytes also
rejects changes to comments/formatting; original row equivalence is not reapplied to excuse
new content at the retained location.

The owning [REST v3 schema](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)
was re-fetched and pinned. Its setup operation is a separate PUT; this inspection does not
invoke it. HSSP setup/journaling, fresh clock mapping,
controller/dispatcher integration, live provider/device and human acceptance remain open.
See [decision](../research/phase-b/ADR-B5-028-fresh-handy-setup-observations.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-inspection/checkpoint.json).


## Scoped verification

The complete Protocol suite passes 313 tests, Engine Handy checks pass 72, coordinator
regressions pass 51, and Client Handy checks pass 18 with one existing isolated desktop
case skipped. Strict default/all-feature workspace lint, App/Worker builds, format and
architecture checks pass with per-run incremental compilation disabled. The final pass
strengthens the actual HTTP download test with equal-length altered bytes and unchanged
CSV rows: it must reject the digest change, not merely a size change. Unaffected Protocol,
Client and coordinator results are retained from the preceding successful source snapshot.

Engine tests verify frozen credential identity, one shared deadline, no durable request
writes, changed downloaded/remote state refusal, and late forgetting, session closure,
registration change and revision change. Local HTTP tests verify GET-only paths, no device
credentials on script download, omission of HSSP reads outside its mode, strict mode
generation decoding and withholding of a numeric credential echoed by the remote state.
The public report cannot claim setup or qualification.

The isolated native CLI campaign verifies discovery, explicit consent, missing-session
refusal, stale revision/old epoch refusal and absence of a durable inspection receipt.
It uses only loopback, joins both Engine processes and leaves worker custody empty. Positive
combined inspections use injected Engine observations and separate local HTTP checks;
this is not a successful vendor/device or rendered-desktop qualification.

An initial formatting delimiter error was corrected before compilation. The short initial
compile check reached its 180-second limit without a source error; the longer validation
completed successfully. No production deadline or assertion was weakened. Qualified setup
and its effect journal, clock/controller integration, physical and
human acceptance, useful models/media and full platform/storage release gates remain open.


## Selecting accepted sample limits

Add `--qualification-id DOSSIER_ID --qualification-revision REVIEW_REVISION` to select a
current [accepted dossier](PULSAR_HANDY_QUALIFICATION.md). The Engine verifies its current
subject and retained evidence before and after I/O. It also requires every verified hosted
sample to meet the declared position range, adjacent step limit and minimum interval.
Limits apply after CSV quantization; equality is allowed. An out-of-envelope script refuses
before any inspection GET. A settings mismatch with the reviewed subject refuses before
the hosted-file GET. Withdrawal during I/O still refuses the late result.

The native hosting reference retains exact source/download identities and a bounded sample
summary; the report remains unqualified and performs no setup. Hardware settings, native
interpolation and physical dynamics are not proved by discrete sample limits. See the
[current checkpoint](evidence/roadmap-2026-10-16/handy-sample-envelope/checkpoint.json).

The [operative-settings guide](PULSAR_HANDY_OPERATIVE_SETTINGS.md) describes the exact
configuration fields, version-2 qualification binding and legacy-record behavior. These
observations are not physical calibration or a hardware stopping proof.

The separate [guarded setup command](PULSAR_HANDY_SETUP_SEND.md) now performs the consented
PUT with its own durable claim. This inspection command remains GET-only and grants no
loaded-program or playback authority.
