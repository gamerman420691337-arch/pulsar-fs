# HSP qualification, emergency history and recovery

Generated HSP streams now have a separate qualification subject, durable emergency
observations and exact custody recovery. These extend the existing qualification
store and recovery workflow. They do not enable public live-stream admission or
establish actual Handy, model-quality or synchronization acceptance.

## Author the correct qualification subject

```sh
pulsar handy-qualification-context --transport hsp
```

This reads the loaded control image, platform and pinned HSP transport identity.
The default remains `hssp`. A client rejects a context for the wrong requested
transport. Neither command contacts a device or creates acceptance.

Subject version 7 binds the existing stroke/dynamics envelope, operative settings,
server/device clock contracts and recovery contract to the seven HSP queue-policy
values. It explicitly declares rate-one playback, no looping or starvation pause,
no append flush and the sole-controller assumption. Versions 1–6 remain hosted
HSSP subjects with unchanged serialized identities. Hosted timing and script checks
reject HSP subjects; HSP checks reject hosted subjects.

Use the existing [qualification workflow](PULSAR_HANDY_QUALIFICATION.md) to retain
the eight evidence roles and explicit operator decisions. Synthetic evidence cannot
receive physical acceptance. Current decision revision, registration, retained
evidence, control image and settings are rechecked. HSP acceptance also requires
a fresh mode-4 observation. Historical mode-4 records without HSP state remain
readable but cannot satisfy HSP qualification or recovery.

HSP observations preserve signed script time and sentinel indices, including
finite pre-Play rates. They are metadata, not synchronized presentation time or
physical stopping proof. Inspection uses bounded fixed-origin GET requests and
withholds credential-bearing output.

## Preserve emergency attempts without inventing ordinary claims

Device journal version 11 adds immutable original qualification links, retrospective
emergency observations and exact HSP resolution membership. It preserves original
v10 stream/effect bytes and Unknown outcomes. Migration does not invent qualification
links for older streams; missing original linkage cannot be repaired by guessing a
dossier from a digest.

An emergency observation retains the exact native Stop, original device/mode/route
context and the fact that no confirmed ordinary prewrite authorized that attempt.
It is distinct from an ordinary claimed command. Repeating its metadata commit
cannot resend Stop. An unused ordinary preparation may be cancelled separately;
that cancellation does not erase the emergency attempt. Conflicting later history
is rejected rather than replacing the original observation.

`handy-hsp-status` reports `emergency_history_durable` only after the exact
retrospective observation is committed. That flag is historical persistence, not
device qualification or proof that movement stopped.

## Keep watchdogs independent of storage

Each session owns at most one metadata worker. Preparation, completion, cancellation,
emergency persistence and membership reads run there. The pump continues its
watchdog and can dispatch the exact native Stop while metadata is held. Native
probe/Stop ownership reads also run on their owned workers, not the pump.

Mutex contention retries the same metadata task and token under one fixed
three-second episode measured from its first attempt. Retries cannot renew that
episode or original source, sample, Play or command deadlines. Poisoned metadata
and actual database failures retire ordinary work. Late metadata never issues a
network command. Shutdown joins metadata workers; a blocked filesystem operation
can prolong the final join, so this is not a physical shutdown-latency guarantee.

## Recover only the captured custody

The existing [reconciliation commands](PULSAR_HANDY_RECONCILIATION.md) select the
explicit accepted transport. HSP and hosted custody are not silently combined
under one transport's qualification.

1. Capture the original immutable plan, projects, aliases, qualification links and
   complete effect/emergency membership.
2. Fence matching native admission. Retire and drain only the captured native
   sessions, including command/probe and metadata workers. Unrelated streams remain
   usable. Cancellation or failure leaves the selected admission closed.
3. Wait the qualified settling interval and obtain two fresh matching mode-4 stopped
   samples under the accepted contract.
4. Recheck acceptance, actor/project scope, identity/settings and exact history,
   including the native drain's sealed memberships.
5. Commit immutable resolution, then release only the captured native owners using
   that exact verified receipt. Superseded drain proofs and mismatched receipts
   cannot release replacements.

Historical-only aliases remain in the committed receipt even when no native session
survives. Reading or replaying a completed recovery restores no authority and sends
no commands. Old session handles stay retired. Later playback requires a fresh
admission and all current preparation, source and clock dependencies.

## Verification and remaining work

The [checkpoint](evidence/roadmap-2026-10-23/hsp-recovery/checkpoint.json) records
exact sources, commands and retained failures. Full Core215, Protocol416 and
Clients565 (14 ignored) pass. Both strict lint configurations and format checks pass.
The full current Engine binary passes 1,425 tests with 72 ignores on controlled RAM;
its ordinary-host run passes 1,423 and fails two native timing cases. Unchanged
isolated repeats of both pass. Earlier runs and their three timing/metadata failures
remain recorded; no general storage-latency repair or qualification is claimed.

The forced late-Prepare regression holds Stop preparation, its cancellation and
emergency persistence in turn. Stop finishes before those metadata workers, the
late unused ordinary row becomes Cancelled, exact retrospective history commits,
and the original single Stop PUT is not repeated. The playback fixture now supplies
consistent fixed clocks during construction; production deadlines are unchanged.

Actual v10/v11 executables verify populated migration, preservation of original
Unknown history, refusal by the preceding reader without changing existing database
or WAL bytes, and current restart. All 14 direct children join. Positive qualification,
recovery and transport tests use authored localhost scenarios. Native migration and
full controlled-RAM checks do not establish ordinary-storage performance, actual
human acceptance, useful motion quality or physical-device behavior.

Public HSP admission still needs genuine native preparation, replay-safe original
intent, a checked accepted-inference producer with the capture/native clock bridge,
and browser controls/presentation. Useful learned motion, real media, Handy 2 Pro
firmware/device measurements, sustained latency and all M1–M8 platform, six-axis,
human and release obligations remain open.
