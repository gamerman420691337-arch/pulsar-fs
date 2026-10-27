# Saved Handy references in Pulsar Desktop

The **Devices** tab now loads owner-scoped saved references and local recovery
records through the existing background request thread. This initial refresh is
local metadata work; it does not contact a device. An unavailable Engine or recovery
store leaves an explicit error and requires a deliberate refresh.

Choose **New reference**, enter a logical device ID, label and two credential-variable
names, then **Save reference**. Select an existing catalog entry to edit it or use
**Disable saved lookup now**. A save/disable acknowledgment triggers a fresh catalog
read: historical receipts are never promoted into current configuration. Saving reads
no Handy key values. Disable affects lookup and sends no device stop command.

To inspect a saved reference, select its current entry, leave its saved fields
unchanged, check **Allow this saved reference's online inspection**, and click
**Inspect saved Handy online**. Draft edits, changed selections and dispatched actions
clear consent and the prior observation. Stale/disabled selections refuse inspection.
The returned firmware/connectivity report remains a timestamped snapshot and grants
no playback authority. The existing direct inspection controls remain available.

## Original-intent recovery

Before a save/disable is sent, the shared client stores its exact ID and immutable
intent in a bounded private session/credential-bound journal. It stores environment
names and nonsecret owner binding hashes, never API-key values or owner capabilities.
The UI freezes a pending change; later draft edits cannot retarget its retry. Loading
recovery metadata never dispatches a mutation or restores inspection consent.

After an unconfirmed result, **Retry original change** repeats that exact request.
**Resolve or withdraw original change** asks the Engine to settle its original identity:

- If it committed, the Engine returns its original historical receipt without changing
  the current registration.
- If it did not commit, the Engine atomically reserves that exact original request as
  withdrawn. Any delayed identical mutation then refuses.

The resolution ID is also retained before dispatch and reused after lost replies.
This operation is not a read-only status query and does not undo an applied change.
Current catalog contents alone never settle an unknown intent. Only an Engine receipt
or durable withdrawal allows **Dismiss settled record** to remove local recovery
metadata; Engine history remains. Journal errors preserve the original intent and
disable new changes until recovery can be read again. Session/credential or owner
binding changes refuse automatic migration.

The CLI also exposes `handy-resolve-registration` with distinct `--request-id` and
`--original-request-id` values plus bounded `--intent-json` containing the captured
`HandyRegistrationIntent`. It uses the same Engine operation and response checks.
Older registration readers cannot decode a withdrawal receipt and refuse its late
original mutation; they cannot replace that receipt with a fresh configuration.

## Evidence and limits

Tests cover receipt/withdrawal races, atomic rollback, restart fencing, original-ID
recovery, private journal binding, local acknowledgment-write failure, stale UI
selections and headless drawing. The native campaign runs the actual panel controller
and shared recovery path against an isolated Engine. It drops a committed reply,
recreates the panel, resolves the saved change, withdraws an unsent intent and checks
late-original refusal with current and preceding binaries. It also verifies consent
is not restored and settled metadata can be dismissed. This is not rendered native
window or human usability acceptance.

Public physical playback remains disabled. Live session/credential ownership,
qualification intake, script setup, controller/transport integration, physical Handy
and six-axis evidence, model/media, platform and human/release gates remain open.
See [decision](../research/phase-b/ADR-B5-023-desktop-handy-intent-resolution.md),
[registration contract](PULSAR_HANDY_REGISTRATIONS.md), and
[checkpoint](evidence/roadmap-2026-10-16/desktop-handy-registration/checkpoint.json).


## Final scoped results

Protocol tests pass 303; all ten Engine registration/resolution tests and nine Client
behavior tests pass. The normally ignored native panel-controller case was run explicitly
and passes against the actual Engine. It recovers an actually dropped committed reply,
restores retained intent without dispatch or consent, resolves applied history, withdraws
an unsent save and dismisses settled local records. The preceding registration reader
refuses the reserved original with an unknown-resolution error; the current reader refuses
it after restart. Both preserve the current registration. Native state contains one
reference and zero projects, jobs, candidates or exports, with joined Engine/proxy work
and empty worker custody.

Strict default/all-feature workspace lint, App/Worker builds, format and architecture
checks pass. The initial test-initializer lint failure is retained. No fresh full Engine,
Client or Core suite is claimed; preceding ordinary-storage/full-suite failures remain
open. Isolated native-window tools were unavailable, so headless drawing and controller
IPC do not establish rendered GUI or human usability acceptance. No real Handy or other
physical device was contacted or qualified.
