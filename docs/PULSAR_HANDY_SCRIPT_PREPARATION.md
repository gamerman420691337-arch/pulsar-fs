# Local Handy Stroke script preparation

`handy-prepare-stroke` publishes a local CSV containing the selected committed Stroke
samples, plus a receipt binding the exact project revision, master hash and file bytes.
It sends no network request, uploads no data and contacts no device. It needs no Handy
credentials. The receipt is an input to future hosting/setup consent, not playback
admission or permission to share the script.
The current development executable is
`/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar`; use that path in place
of `pulsar` below if it is not installed on your PATH. These controls are currently CLI-only.

```sh
pulsar handy-prepare-stroke PROJECT /absolute/new-script.csv \
  --revision REVISION --request-id ORIGINAL_REQUEST_ID
```

Use an existing committed project and its exact revision. The destination must be new.
The Engine checks both Read and Export grants. Preparation uses the existing durable
export reservation and no-clobber publication path. A repeated original ID returns its
historical receipt without overwriting or recreating the destination. A changed original
intent conflicts. If publication or its acknowledgment has an uncertain outcome, the
original remains unknown and is not automatically published again.

The receipt includes SHA-256/byte length, action count, first/last timestamp, maximum
quantization error, explicitly omitted non-Stroke axes, and unresolved/unknown review
status. The CSV contains only a fixed format comment and numeric rows, with no project
name, media path, provenance, model details or credentials. Future hosting must re-read
and verify the actual file bytes against the receipt and obtain separate explicit
consent for the exact artifact and destination. A historical receipt does not prove the
file still exists or has not been changed externally.
Portable project capture keeps the inert Stroke/hash/export-state record; it does not
resolve the archival path, copy the published output bytes, or restore hosting permission.

## Encoding contract

The format is a comment line followed by decimal `time_ms,position_percent` rows with
LF line endings. Times round to the nearest millisecond, half upward; normalized positions
round to integer percentages. Timestamps remain strictly increasing. Negative time,
missing/empty/single-point Stroke tracks, unresolved Stroke gaps and millisecond
collisions refuse; preparation never inserts points, changes cadence or fills gaps.
Other axes are explicitly listed in the receipt and do not enter this single-axis file.

Maximum timestamp error is reported in nanoseconds. Position error is a conservative
ceiling in millionths of normalized full travel; floating-point representation can make
a mathematical half-percent tie report 5001 rather than 5000. Local resource limits are
4 MiB of output and one million input actions. These are operational limits, not measured
Handy buffer, firmware, travel or timing limits.

These are neutral samples. Pulsar's analytic device-motion proposal assumes rest-to-rest
quintic interpolation. A plain script cannot establish that a Handy executes that curve,
so this preparation does not reuse its velocity/acceleration/jerk bounds or call the CSV
physically feasible. Actual interpolation, quantization, entry/exit, synchronization,
ordering and stopping require qualification for the exact device/firmware/profile.
The receipt always retains `release_qualified: false`.

## Owning format evidence

The pinned [manufacturer SDK 2.3.1](https://unpkg.com/@ohdoki/handy-sdk@2.3.1/dist/handy.esm.js)
converts Funscript action timestamps and positions to rounded decimal CSV pairs and a
comment header. Its format conversion is the compatibility reference here; its old API-v2
transport is not adopted. The [Hosting API v2 schema](https://www.handyfeeling.com/api/hosting/v2/docs/spec.yaml)
accepts CSV uploads and returns a temporary URL; downloaded content is CSV. Its sample
URL versions and identifier examples are inconsistent, so future hosting must validate
returned URLs and downloaded content rather than assume byte identity. No hosting
endpoint was exercised with script data in this change.

[REST v3](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml) specifies URL-based
HSSP setup. A local path is not a valid cloud handoff. Hosting disclosure, setup, loaded
stream binding and qualified controller/transport ownership remain separate unfinished
work. See [connection sessions](PULSAR_HANDY_CONNECTION_SESSIONS.md),
[decision](../research/phase-b/ADR-B5-025-exact-stroke-script-preparation.md), and
[checkpoint](evidence/roadmap-2026-10-16/handy-script/checkpoint.json).


## Scoped verification

The full Protocol suite passes 305 tests. Engine Handy tests pass 53; shared export
regressions pass 31 with one existing preceding-reader qualification case skipped.
Client Handy checks pass 16 with one existing isolated desktop case skipped. Strict
default/all-feature workspace lint, App/Worker builds, format and architecture checks pass.
These scopes overlap and are not a full Engine/Client/Core suite claim.

The isolated native CLI creates the deterministic two-second demo and publishes its
17 rows. It recovers an actually dropped committed reply, preserves externally changed
output on historical replay, refuses a new write over that output, and retains an
acknowledgment-write failure as unknown across restart. The master revision and unresolved
review flags remain unchanged. Both Engine processes and the proxy are joined; worker
custody is empty. No credentials, model/runtime assets or real media are supplied, and
networking is isolated.

A second minimal sandbox loads the hash-pinned, unchanged manufacturer SDK and compares
its CSV data rows and a fractional rounding vector. All 17 rows and the rounding vector
match; format comments deliberately differ. The browser-oriented module initially refused
to load in Node because `location` was absent. A synthetic browser-origin object permits
loading the unchanged module for this pure conversion check. No SDK instance, browser UI,
network operation, hosting or physical qualification is exercised.

Initial compilation name shadowing, the format/axis archival integration error and the
negative-time fixture failure are retained and corrected. The latter now asserts Core's
nonnegative-time boundary rather than trying to construct an invalid action. No production
guard or deadline was weakened. The added actual package-capture regression passes with
Stroke ledger metadata and does not resolve externally changed output bytes.

Full ordinary-storage suites, live hosting/setup/dispatch integration, physical firmware,
interpolation, timing/stopping, model/media, platform and human/release gates remain open.
The separate analytic device-motion archival-axis finding recorded in this packet was
subsequently reproduced and repaired through [archival scope and package v14](PULSAR_DEVICE_EXPORT_ARCHIVES.md).
That later repair has its own source identities and verification; the CSV checkpoint remains
an unchanged record of this earlier increment.
