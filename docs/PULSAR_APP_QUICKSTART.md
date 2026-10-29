# Use the current Pulsar app


2026-10-24. Development build; full roadmap PARTIAL. The desktop and CLI share a local Engine.
Model-free patterns, script import/editing and export are available. Media analysis requires an
explicit model/profile and runtime; useful trained motion and the full platform matrix remain
unqualified. See [current status](PULSAR_CURRENT_STATUS.md) for the latest verified scope.

Scoped Handy CLI operations now cover setup, preparation, controller leases, timing,
[guarded play/status/cancellation](PULSAR_HANDY_PLAY_SUBMISSION.md),
[stop history](PULSAR_HANDY_EMERGENCY_STOP.md), and
[qualified command recovery](PULSAR_HANDY_RECONCILIATION.md). They require exact current
configuration, evidence and authority; the software tests do not qualify a real device. Desktop
Arm and the broad physical-playback capability remain disabled. The default browser preview has
no device linkage. An optional [player stop guard](PULSAR_BROWSER_PLAYER_CONTROL.md) can retire
selected existing playback on player loss; it does not start or synchronize the device.
[Continuous timing refresh](PULSAR_HANDY_CONTINUOUS_TIMING.md) has scoped CLI/software validation.
Browser-generated HSP streaming now passes its positive localhost demo; fault
coverage and physical synchronization remain unfinished.

## Launch on this workstation

```sh
/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar gui
```

The companion `pulsar-worker` is beside this executable. No arguments also opens the desktop. This is the current development build; an older installed `pulsar` or retained runtime candidate may not contain these controls. Model-free patterns and script editing do not need a neural model. Media preview requires configured FFmpeg/FFprobe; visual inference requires the admitted model/profile and ONNX runtime. A running Engine keeps its existing startup configuration until it is restarted deliberately.

## Desktop demo without models, media or hardware

1. Enter a project name in the left **Project** panel and click **Create project**.
2. Open **Generator** and select **Explicit axis patterns**.
3. Paste this single line:

   ```text
   pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8
   ```

4. Click **Generate axis-pattern candidate**. The committed timeline remains unchanged while the Engine generates the candidate.
5. Click **Inspect candidate and protected regions** and review its synthetic provenance and diagnostics. The expected pattern spans two seconds, centered at 50%, reaching 40% and 60%.
6. Click **Commit whole reviewed candidate** to replace the new empty project's master with the reviewed candidate. For existing authored work, inspect protection and use the selective merge controls deliberately.
7. Select Stroke on the timeline. Enter a new absolute destination in **Export Stroke .funscript path** and click **Export committed revision**.
8. Inspect the exported JSON: 17 actions at 125 ms intervals, from 0 through 2000 ms. Undo/Redo operate on retained project history; exported files are separate snapshots.

This is a deterministic authored pattern. It neither analyzes a video nor drives a device. These button labels and the shared production pattern grammar are checked against current source; the native CLI equivalent below is executed. The pinned pre-format15 Linux app also passes an actual rendered walkthrough of create/generate/review/commit/export, overwrite refusal, Undo/Redo and joined Engine restart/reopen. The exported 17 actions match an independent oracle. See [the retained receipt](/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-28/rendered-pattern/verification.json). This is scoped software evidence. The [execution dashboard](PULSAR_MILESTONES.md) records current build/browser checkpoints and remaining physical-device acceptance obligations.

## Equivalent checked CLI demo

```sh
/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar synthesize 'pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8' --output /absolute/new-demo.funscript
```

Unlike the desktop's staged workflow, this convenience command creates a project, generates, commits and exports in one operation. It prints the retained project ID to stderr. You can enter that ID in **Existing project ID**, then click **Open project**. The destination must be new. Repeating against an existing destination refuses and preserves its bytes, but can retain another attempted project's history.

The retained 2026-10-13 native campaign ran its recorded executable and companion with networking disabled and no model/runtime environment configured. The exported actions exactly match an independently listed triangle oracle; repeating the command refuses and preserves the original file. The owned Engine is joined and private temporary state is removed. See [results](evidence/roadmap-2026-10-13/user-quickstart/native-results.json). This does not establish learned motion or device compatibility.

## Import an existing script or video

For a script, open **Generator**, select **Funscript candidate import**, enter its absolute path and click **Import script candidate**. Inspect the candidate, then separately commit or merge. Original script bytes remain separate from project edits.

For media, enter an absolute file path in **Media path** and click **Import immutable source**. Use **Studio** or **Cinema** for preview. For generation, choose **Video motion** and an explicit admitted model/profile or selected pack; no useful default model is currently selected. Do not interpret a diagnostic graph's output as recovered source motion.

## Select captured CPU dependencies

For explicit CPU generation or preview, the Linux x86_64 Engine can load a private
`PULSAR_CPU_OWNER_POLICY` at startup. This selects exact captured library bytes and
keeps their runtime identity through job history and resume. It can select an ORT
library independently of the default runtime. See the
[captured CPU guide](PULSAR_CAPTURED_CPU_RUNTIME.md) for configuration and limits.
This mode does not yet support live CPU streams or portable history. It provides
a runtime prerequisite for the separate opt-in fallback path below and does not
qualify a useful model.

## Opt-in equivalent backend fallback

This software path passes unit and strict lint checks. The native software
simulation passes eleven scenarios, including direct-model and selected-pack
successors, cancellation/withdrawal/expiry before continuation, invalidation after
verified active progress, and pack withdrawal before and during a successor.
Each successful path executes 199 actual CPU inference calls. Eligibility,
initial provider absence and pack-owner approval are authored fixtures. No real
measured comparison or genuine owner acceptance has been created, so there is no
installed qualified fallback to demonstrate yet. Ordinary generation remains
strictly pinned unless you explicitly select a fallback policy. See the
[verification receipt](/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-24/fallback-active-consumers/verification.json).

In **Generator → Equivalent backend fallback**, enter the absolute **Selection
policy JSON (absolute path)**, then check **Allow accepted equivalent backends for
this generation** and use **Generate stroke candidate**. The checkbox requires an
available capability, decoded-media source, explicit model/profile or current
selected pack, and a valid policy path. It resets after submission or binding
changes. Review and commit remain separate desktop actions.

The corresponding CLI shape is:

```sh
pulsar generate /absolute/video.mp4 \
  --model /absolute/model.onnx \
  --inference-profile /absolute/profile.json \
  --fallback-selection /absolute/owner-reviewed-fallback.json \
  --output /absolute/new-output.funscript
```

Alternatively, replace `--model` and `--inference-profile` with
`--model-pack CURRENT_MANIFEST_SHA256` for the current owner-selected pack. The
CLI convenience command commits and exports a successful candidate; use a new
output path. This is a schematic invocation, not a verified fallback demo.

A selection file has this schema. Replace the two placeholders and illustrative
revision `1` with exact values from a current reviewed record; this template is
not evidence or an acceptance and needs real record values before use. The report
digest binds the exact canonical report, not its formatted CLI output.

```json
{
  "schema_version": 1,
  "equivalents": [{
    "dossier_id": "<CURRENT_DOSSIER_ID>",
    "expected_revision": 1,
    "expected_report_sha256": "<EXACT_CURRENT_REPORT_SHA256>"
  }],
  "maximum_fallbacks": 1,
  "source_policy": {
    "schema_version": 1,
    "kind": "decoded_media",
    "sources": null,
    "independent_audio": {"kind": "forbidden"}
  },
  "view_policy": {
    "schema_version": 1,
    "applicability": {"kind": "preserve_requested_semantics"}
  }
}
```

The source/view policies must match the accepted dossier's scope. Here,
`sources:null` explicitly permits any decoded-media source and independent audio
is forbidden; it does not infer a restriction from evaluation or training media.
A source allowlist instead uses exact content identities. The view policy keeps
requested target, view, timing, quality tier and generation semantics unchanged.
Reference order is significant: at most 32 records and 1–8 fallback transitions,
with the transition limit no greater than the reference count.

The Engine requires an actual configured captured CPU or CUDA runtime, current
accepted measured evidence and matching environment/scope. Synthetic, missing,
stale, withdrawn or expired records cannot supply that admission. Only confirmed
provider unavailability after worker retirement can schedule a fresh attempt;
malformed models or arbitrary errors cannot. Cancellation, revocation, withdrawal and expiry
invalidate attempt authority and fence publication. Restart restores history,
without automatically continuing pending fallback or restoring runtime permission.

Inspect attempts with the read-only controls below. A selection file and retained
history cannot authorize execution. This mode does not support raw-frame pairs,
browser/live fallback or portable fallback history, and it does not qualify
useful motion or physical playback.

## Inspect generation attempts

For a generation job's failure details, use **Generator → Inspect attempt history**.
This shows its explicit profile, original typed diagnostic and recorded worker
cleanup status without retrying. The matching read-only CLI is
`pulsar job-attempts --project PROJECT_ID --job JOB_ID`.
See [attempt history](PULSAR_INFERENCE_ATTEMPT_HISTORY.md) for its limits. History
alone cannot authorize retry, fallback or candidate publication.

## Inspect inference equivalence records

`pulsar model equivalence list` and `status DOSSIER_ID` inspect retained comparisons.
The [equivalence workflow](PULSAR_INFERENCE_EQUIVALENCE.md) documents Pending imports,
explicit owner decisions, withdrawal and original-request recovery. The complete
local CLI lifecycle has been exercised with synthetic evidence; no real comparison
has been accepted. Use the desktop's **Model evidence** tab for the same record
inspection, explicit decisions and original-request recovery. Selection changes
clear acceptance consent; Synthetic records cannot be accepted. Its state/headless
checks pass, while a rendered desktop walkthrough is still pending.

`pulsar model equivalence environment --profile /absolute/profile.json` produces
an actual local environment observation for a configured runtime. The
[inspection guide](PULSAR_INFERENCE_ENVIRONMENT.md) explains its byte checks and
remaining complete-runtime-dependency gap. Neither an environment report nor
a registry decision alone authorizes the opt-in execution path above, and neither
workflow enables physical playback.

## Prepare video/script reference sets

The Linux desktop's **Reference sets** tab pairs `Live-Reference` videos with
scripts and source-bound target views. Add pairs, open a selected source in
preview, adjust its signed script offset, then export to a new folder. The
read-only curve uses the displayed frame's actual source time. Saved sets can be
reopened with content checks; neither preview nor export marks alignment reviewed.
See [the reference-set workflow](PULSAR_REFERENCE_SET_BUILDER.md) for drawing a
target view, the exact offset convention and the local CLI equivalents.

## Browser and Handy status

The [HSP live-analysis controls](PULSAR_BROWSER_HSP_LIVE.md) have a recorded complete
40-second browser-to-simulator demonstration, including two source transitions
and final Stop. The latest sustained rerun stopped during startup on an intermittent
heartbeat timeout; this remains a development workflow. Delayed-Bind cancellation
now has a separate passing browser/simulator case. This uses an authored neutral
model and tests software behavior. Useful video
motion and physical Handy 2 Pro synchronization still need qualification. The
public sequence is Inspect → Prepare synchronization → Bind analysis → enable
configured continuity → Play; each device action has separate consent. Default
browser launch remains an unlinked preview.

To play an imported project source in the browser, first add its active unit-speed
placement. Then use **Refresh Engine sources → Prepare selected source → Play** in
the browser page, with an explicit declared clock acknowledgment. This preserves source
and project-time provenance through recordings. Preparation and pause do not start a
device. See [the source-media workflow](PULSAR_BROWSER_SOURCE_MEDIA.md) for exact steps,
limits and the rendered diagnostic demo.

The browser starts **Requested lead** at half the configured analysis-age budget,
with a 1 ms minimum and a 1,000 ms cap. For a 1,000 ms age budget, it starts at
500 ms. You can change the request before playback. A shorter lead leaves more
freshness margin but less time for analysis. Native admission and each estimate's
original expiry still apply; this setting does not enlarge the age budget or
establish synchronization.

Open **Browser live** in the desktop, choose **Generator model/profile** and **Capture Generator model/profile** after loading a compatible explicit file-model profile in Generator, or choose **Owner-selected pack** and **Capture owner-selected pack** after refreshing its selection, or enter a live-options JSON path. Review the live settings, click **Launch browser player**, then click **Open local browser player**. The launcher uses the open project and revision and owns a finite server lifetime. **Stop browser player** or a project/revision change requests capture cleanup and joins the server. See [desktop launcher](PULSAR_DESKTOP_BROWSER_LIVE.md) and [direct configuration steps](PULSAR_DESKTOP_LIVE_OPTIONS.md). The [CLI local browser player](PULSAR_BROWSER_LIVE_PREVIEW.md) remains available with an explicit project/current revision. It supports local video preview, accepted-estimate recording, candidate review/commit, explicit gap repair and gap-free Stroke download. The player also offers [Use camera](PULSAR_BROWSER_CAMERA.md), with explicit video-only permission and the same recording workflow. Camera input and experimental tab sharing retain capture-relative timestamps; it does not establish source-player timing. Useful motion inference and synchronized Handy output are unfinished. The optional [Handy stop guard](PULSAR_BROWSER_PLAYER_CONTROL.md) adds exact existing-play monitoring and stop requests, with independent heartbeat handling. It does not provide synchronized start or generated device streaming. Default launch has no device linkage.

**Devices** provides [read-only Handy inspection](PULSAR_DESKTOP_HANDY_INSPECTION.md). Credentials must be encrypted and supplied by environment name. Neither connectivity inspection nor exported scripts qualify Handy 2 Pro playback. Physical qualification still needs actual firmware, credentials and measured stopping/synchronization evidence.

The [guarded native playback CLI](PULSAR_HANDY_PLAY_SUBMISSION.md) adds `handy-play`,
`handy-play-status` and exact unclaimed `handy-play-cancel`. New play requires accepted
qualification and current native controller/timing state. These commands have local software
tests; no actual device qualification or browser synchronization is claimed. Cancellation of
unclaimed work is distinct from stopping attempted playback. Desktop Arm remains disabled.

See [current status](PULSAR_CURRENT_STATUS.md), [software specification](PULSAR_SOFTWARE_SPECIFICATION.md) and [major roadmap](PULSAR_MAJOR_ROADMAP.md) for the full scope and acceptance gates.


## Saved Handy references

The rebuilt CLI supports `handy-register`, `handy-registrations`, `handy-disable`
and consent-bound `handy-inspect-registered`. Registration can be prepared before
credentials exist: it stores environment names only and contacts no device. See the
[registration guide](PULSAR_HANDY_REGISTRATIONS.md) for exact commands and recovery.
In **Devices**, use **Saved Handy references** to refresh, save or disable a reference.
Inspection requires a current saved selection and a new consent checkbox. Pending
changes offer original retry or resolution/withdrawal. See the
[desktop recovery guide](PULSAR_DESKTOP_HANDY_REGISTRATIONS.md). These references do
not arm or qualify a device; physical playback integration remains open.

## Explicit Handy connection observations

The CLI adds `handy-sessions`, `handy-open-session` and `handy-close-session`.
An open requires the Engine epoch from `handy-sessions`, the saved registration's
revision/hash, an original request ID and explicit inspection consent. It retains
one native credential snapshot and observes connectivity/identity; it does not send
motion. Exact retry uses the captured pins, and restart refuses the old Engine epoch.
Close only retires this local observation. See the
[connection guide](PULSAR_HANDY_CONNECTION_SESSIONS.md) for commands and limits.
Session controls are currently CLI-only; the desktop's saved-reference controls remain
available separately. Qualified synchronized playback is still unfinished.


## Prepare a local Handy Stroke CSV

For a committed project, the CLI can now prepare a Stroke CSV without credentials or
network access:

```sh
/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar handy-prepare-stroke PROJECT /absolute/new-script.csv --revision REVISION --request-id ORIGINAL_ID
```

It reports exact output bytes, the source revision/hash, rounding error, omitted axes
and unresolved review status. The destination must be new. Exact retry returns the
historical receipt; it does not restore an externally changed file. This is local sample
preparation, not hosting, device setup or qualified playback. See the
[preparation guide](PULSAR_HANDY_SCRIPT_PREPARATION.md).


## Explicit temporary public hosting

The CLI adds `handy-host-stroke`, `handy-hosting-status` and `handy-forget-hosting`.
Hosting shares the exact prepared CSV with Handy's public temporary hosting service;
it requires separate consent, the original preparation ID/hash, current revision and
Engine epoch. It does not load or play the script on a device. Status is read-only;
forgetting retires a local link and does not delete the server's copy. Unknown outcomes
must not trigger automatic new uploads. Follow the [hosting guide](PULSAR_HANDY_HOSTING.md)
for the complete command and recovery contract. Vendor service qualification and actual
synchronized playback are still unfinished.


## Read-only setup inspection

`handy-inspect-setup` checks a current project revision against an observed connection
session and a verified hosting result. It requires fresh explicit online-inspection
consent, rechecks the hosted file, and samples identity/mode/remote state around that
check. Changed or retired references refuse. No upload, mode change, setup, play or stop
is sent. The report remains unqualified and cannot authorize later playback. See the
[inspection guide](PULSAR_HANDY_SETUP_INSPECTION.md) for the exact CLI arguments and limits.


## Handy qualification evidence

The CLI can retain evidence with `handy-qualification-import`, inspect it with
`handy-qualification-status`, and record a separate operator acceptance or withdrawal.
Start with `handy-qualification-context` to capture the running control build and platform.
Import leaves a new dossier pending; synthetic evidence cannot receive physical acceptance.
A selected accepted dossier can be checked by `handy-inspect-setup`, which still performs
no setup or playback. Follow the [qualification guide](PULSAR_HANDY_QUALIFICATION.md)
for the required evidence, explicit review consents and exact command arguments. Real
Handy qualification remains unfinished. Selecting a dossier during setup inspection now
checks the quantized script against its position, step and interval limits; physical dynamics
remain separate checks. Version-2 qualification subjects now bind observed slider settings
and stroke limits; see the [operative-settings guide](PULSAR_HANDY_OPERATIVE_SETTINGS.md).


## Read setup history

`handy-setup-status PROJECT --setup-request-id ORIGINAL_SETUP_REQUEST` resolves this client's
original setup history without a saved receipt ID or a live device connection. It requires
the local Handy owner capability and project Read access. The result is historical and
never authorizes setup or playback. See [setup recovery](PULSAR_HANDY_SETUP_RECOVERY.md).
The [guarded setup command](PULSAR_HANDY_SETUP_SEND.md) can now create setup history after
its qualification and preflight checks. Setup acknowledgment does not enable playback.


## Explicit device setup

`handy-setup` sends one selected hosted script only after explicit consent, accepted
qualification and fresh preflight. It does not change mode or start playback. Follow the
[setup guide](PULSAR_HANDY_SETUP_SEND.md) for all required IDs and recovery limits; actual
Handy qualification, loaded-program verification and synchronized playback remain unfinished.

## Stopping the Engine

[Engine shutdown](PULSAR_ENGINE_SHUTDOWN.md) handles SIGTERM and SIGINT, including Ctrl+C for
an Engine running in the foreground. It closes IPC and joins owned work while retaining device
uncertainty. Closing a desktop or CLI client continues to leave the separate Engine available for
retained offline work. Native evidence is Linux-only and does not prove physical Handy stopping.

## Read known stop activity

Use `pulsar handy-stop-status PROJECT --acquisition-request-id ORIGINAL_ACQUIRE_ID` to inspect
known stop history and retained native activity. This read-only command sends no device request.
It distinguishes emergency fallback, actual transport outcomes and pending outcome metadata;
history is explicitly incomplete and never proves physical stopping. See the
[stop fallback guide](PULSAR_HANDY_EMERGENCY_STOP.md).
