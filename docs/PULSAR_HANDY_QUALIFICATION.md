# Handy qualification dossiers and operator review

Pulsar can retain bounded evidence dossiers, record a separate local operator acceptance
or withdrawal, and match a selected accepted dossier during fresh setup inspection.
Importing evidence never accepts it. Synthetic evidence cannot receive physical acceptance.
No real acceptance record or hardware qualification was created while implementing this flow.

Acceptance records an authenticated local operator declaration. It is not independent proof
of human presence, manufacturer certification, or the truth/adequacy of measurements. The
operator must actually review the physical evidence. Hashes establish byte correspondence;
field validation and coverage labels do not turn synthetic results into physical evidence.

## Exact subject

A dossier binds the logical device and registration revision/fingerprint, the complete
observed Handy identity, a physical-unit label, the running control image, platform and
transport contract, and an explicitly declared native HSSP profile. The control image is
not the analysis worker: on Linux it is hashed through the kernel's `/proc/self/exe` handle,
including when its old pathname has been replaced. Platform binding includes architecture
and kernel release. Verified loaded-control-image identity on other platforms remains open;
these qualification operations refuse there rather than hashing a possibly replaced path.

The profile supplies the Stroke/mm dynamics declarations, allowed percentage range and
step, minimum sample interval, command-age and clock-error limits, drift allowance and an
optional physically supported stopping bound. No physical thresholds are supplied by
default. These declarations need real evidence for the native HSSP behavior; they do not
inherit the analytic adapter's quintic-curve proof. When an accepted dossier is selected,
setup inspection checks the chosen script's quantized sample range, step and interval limits.
Physical dynamics and all operative hardware settings remain admission responsibilities.
A stopping bound does not grant headless playback.

Subject version 3 adds an explicit [server-clock contract](PULSAR_HANDY_SERVER_CLOCK.md).
Versions 1 and 2 remain readable without inferred clock-origin or error bounds. A version 3
contract changes the dossier and timing-configuration identity and needs its own actual review.

Subject version 4 adds an explicit [device timing contract and combined error budget](PULSAR_HANDY_PLAYBACK_CLOCK.md).
Its user offset, error, maximum synchronization age, RTT applicability ceiling and drift require
actual evidence. The offset is checked at synchronization preflight and again during combined
clock acquisition. Earlier subjects receive no default device timing bounds.

Eight evidence roles are mandatory: travel/dynamics, native curve/quantization,
timing/synchronization, stopping, disconnect behavior, controller loss, Engine death and
an emergency procedure. A consolidated report may cover several roles, but the operator
must assess the actual coverage. Evidence files are opaque retained data, never executable
input. Keep credentials and other secrets out of the supplied evidence and notes.

## CLI workflow

Use `/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar` in place of `pulsar`
if the development build is not on PATH. The controls are currently CLI-only.

```sh
pulsar handy-qualification-context
pulsar handy-qualification-import INPUT.json --manifest-sha256 SHA256 --request-id IMPORT_ID
pulsar handy-qualification-status DOSSIER_ID
```

Author `INPUT.json` using `HandyQualificationInput` in
[the protocol contract](../crates/pulsar-protocol/src/handy_qualification.rs): `subject`
uses `schema_version: 2` and contains the explicit bindings, profile and observed
`operative_settings`; `evidence` has one entry per required kind,
with an absolute local `path`, expected `sha256` and `byte_len`. Use the returned context,
current registration and actual observed identity. `origin` must truthfully distinguish
`synthetic` from `physical`; changing that label alone is not measurement or acceptance.
Hash the exact input file and retain the original request ID and arguments for recovery.

The Engine reads bounded files, checks every declared identity, and commits the canonical
path-free manifest, evidence bytes and import acknowledgment in one transaction. Original
paths are not used afterward. New dossiers are pending. Importing an identical dossier
preserves its current review state; it does not reset a withdrawal. Inspect the original
selected files against the reported hashes when reviewing; do not rely on filenames alone.

Only after genuinely reviewing the exact physical evidence, an operator may issue:

```sh
pulsar handy-qualification-accept DOSSIER_ID --revision REVIEW_REVISION \
  --engine-epoch ENGINE_EPOCH --device-session DEVICE_SESSION \
  --note 'Your actual review basis' --request-id ACCEPT_ID \
  --acknowledge-physical-evidence --allow-online-inspection
```

Both consents are required independently. Acceptance rechecks retained bytes, the current
control/platform/registration binding and two consistent identity/settings reads through the selected
connection's existing credential snapshot. It checks them again after I/O. Changed review
revision, withdrawal, registration, identity, connection or authority prevents a late
acceptance. All decisions use checked revisions and commit atomically with their receipts.
Neither acceptance nor withdrawal sends setup, play or stop.

```sh
pulsar handy-qualification-withdraw DOSSIER_ID --revision REVIEW_REVISION \
  --note 'Your withdrawal reason' --request-id WITHDRAW_ID
```

Withdrawal does not require readable payload evidence, a current connection or the old
control build; corrupted payload bytes must not trap a decision in accepted state. It
revokes the record for new matching/admission. It is not a physical stop command or a
repair of damaged evidence. Status verifies payloads and can still report corruption.
Historical mutation replay returns its original acknowledgment and never overwrites a
newer review. Query current status before relying on a saved acknowledgment.

## Matching without admission

A current accepted record may be selected on the existing read-only setup inspection:

```sh
pulsar handy-inspect-setup PROJECT --revision PROJECT_REVISION \
  --engine-epoch ENGINE_EPOCH --device-session DEVICE_SESSION \
  --hosting-request-id HOSTING_ID --request-id INSPECTION_ID --allow-online-inspection \
  --qualification-id DOSSIER_ID --qualification-revision REVIEW_REVISION
```

The Engine rechecks the accepted record revision, canonical identity, decision receipt,
evidence hashes, actual control image/platform, registration and observed device identity
before and after inspection. It also compares observed operative settings with the reviewed
version-2 subject. Withdrawal during I/O cannot produce a successful match.
The optional `accepted_qualification` result identifies that configuration-record match.
It is not a client-supplied grant: `qualified` and `setup_performed` remain false. The same
check applies the accepted sample limits to the exact verified hosted rows. Position limits
are inclusive, every adjacent step is checked in either direction, and each interval must
meet the declared minimum. Checks use the exported integer percentages and milliseconds,
including export rounding, rather than the unquantized master.

The native hosting reference binds both the original source-script identity and the exact
download identity. The sample summary is derived while verifying their row equality; a fresh
download must still match the retained bytes. Editing the saved hosting history cannot bind
that native reference to another source script. An out-of-envelope selection refuses before
network I/O, and acceptance/current-reference checks repeat afterward. No new wire fields,
restorable authority or automatic acceptance are introduced.

These are discrete sample checks, not bounds on motion between samples. Interpolation,
velocity/acceleration/jerk, entry from the current device position, loop seams, operative
rate/looping, live hardware/settings, physical stopping,
loaded-stream association and controller admission remain separate work. Public physical playback is
still disabled, and no acceptance or live authority is restored from a project package.

## Storage and limits

Host-local tables have a strict self-versioned layout, validated through a read-only
connection before Engine startup mutations. Unknown layouts/versions refuse. Decisions
must agree with their original request receipts. Payload hashes are freshly checked for
status, acceptance and matching; a stored accepted label alone is insufficient.

Operational caps are 128 KiB for the input document, 4 MiB per evidence role, 32 dossiers
and 256 MiB of total retained evidence. Import reserves memory and checks storage capacity.
Evidence and review history are retained, including after withdrawal; automatic deletion
or quota reclamation is not implemented. The limits do not define physical qualification
criteria. Source files remain separate and are not modified by import.

See [decision](../research/phase-b/ADR-B5-029-bound-handy-qualification-review.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-qualification/checkpoint.json).
Real physical evidence/acceptance, qualified setup and its journal, script admission,
controller/stopping integration and all model/media/platform/release gates remain open.

The [sample-envelope checkpoint](evidence/roadmap-2026-10-16/handy-sample-envelope/checkpoint.json)
records the current implementation and its scoped validation.

[Operative settings](PULSAR_HANDY_OPERATIVE_SETTINGS.md) are now explicit version-2 subject
data. Version-1 history remains readable and withdrawable, but cannot receive new acceptance
or supply settings-aware matching. No defaults or automatic review upgrades are applied.

The [setup sender](PULSAR_HANDY_SETUP_SEND.md) now consumes these accepted facts alongside
fresh preflight and a durable single-use claim. Physical evidence must cover the exact
setup behavior for the current control build. Transport acknowledgment still establishes
no loaded-program authority, physical stopping proof or playback admission.
