# Handy operative settings and qualification subjects

Read-only setup inspection now includes the slider configuration and stroke limits.
It adds GETs for `/settings/slider` and `/slider/stroke` under the existing consent,
credential snapshot and shared deadline. No settings-write operation is implemented.
The report's `device.operative_settings` contains the observed values.

## Evidence binding

New qualification subjects use `schema_version: 2` and explicitly include
`operative_settings: { "slider": { ... }, "stroke": { ... } }`. Use actually observed
values that were covered by the physical evidence; do not copy the software test fixture
as device evidence. The slider object includes all 19 declared fields, including inversion
and overclock flags; stroke includes relative and absolute minimum/maximum positions.
The [protocol types](../crates/pulsar-protocol/src/handy_settings.rs) define their exact
shape. Structural validation checks completeness, finite numbers and ordered ranges;
it does not certify measured thresholds or physical behavior.

The existing [read-only setup inspection](PULSAR_HANDY_SETUP_INSPECTION.md) can obtain
settings without a qualification selection, once its project, connection and hosting
prerequisites are met. Device onboarding tools are a separate source for operator review.
Acceptance makes two fresh bounded observations and requires consistent identity, mode,
settings and remote state. It then compares the settings with the reviewed subject and
rechecks current local authority. A setting that changes after these observations is not
ruled out by their agreement.

Selecting an accepted dossier during setup inspection checks settings before the hosted
file GET and checks current acceptance again afterward. A changed setting, withdrawal or
retired reference refuses the late result. The native timing configuration also binds the
settings. The public report still has `qualified: false` and `setup_performed: false`; it
is neither a motion grant nor proof of stopping.

## Existing dossiers

Version-1 subjects have no settings declaration. Their original hashes, receipts, status
and withdrawal remain readable, but new acceptance and settings-aware matching require a
version-2 subject. No settings or approval are inferred for an old record. Reimporting a
new explicitly reviewed subject creates a new dossier identity; history is not rewritten.
The SQL layout remains unchanged. A preceding qualification-aware reader rejects the new
subject rather than silently ignoring its settings.

## Contract and limits

The [manufacturer REST v3 schema](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)
was fetched again and matches the existing pinned SHA-256
`9ad90e385ff1bf5f454a9af5f02862ca6cb064654b30df873d7d7389198cb684`.
Its slider schema requires `x_limit_stop`; the endpoint example instead says `x_limit_end`.
Pulsar requires the schema field and rejects the example alias and unknown configuration
fields. Actual firmware compatibility with this declared shape remains unqualified.

Exact settings correspondence does not establish physical calibration, interpolation,
velocity/acceleration/jerk limits, entry or loop transitions, active playback rate, thermal
behavior, stopping or controller-loss behavior. The slider's reported regulator timeout is
not promoted to an Engine-death watchdog. Setup, loaded-stream/controller and clock
integration and actual hardware/human acceptance remain open.

See [ADR B5-030](../research/phase-b/ADR-B5-030-operative-settings-qualification-subject.md)
and the [scoped checkpoint](evidence/roadmap-2026-10-16/handy-operative-settings/checkpoint.json).

## Scoped verification

Protocol checks pass 317; Handy Engine checks pass 88; Handy Client checks pass 19 with one
existing isolated-desktop test skipped. Both strict workspace lint configurations, builds,
formatting and architecture guards pass. Loopback HTTP checks exercise the settings GETs,
changed observations, required-field/alias refusal and numeric credential-echo withholding.
Native CLI campaigns cover both subject versions and prove the preceding reader's rejection
of version 2 with database/WAL bytes unchanged. All fixtures remain synthetic.
