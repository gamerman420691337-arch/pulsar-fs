# ADR B5-050 — Independent browser control and native player stop guards

Keep browser liveness independent of inference, editor work and project database access.
A narrowly scoped native guard may attach to an already acknowledged Playing controller,
without granting play or claiming browser/device synchronization. Native challenge issuance
anchors freshness; an old response cannot acquire a fresh deadline merely by arriving later.

The [browser timing research](browser-player-timing-and-liveness-2026-10-22.md) establishes why
reported frame timestamps and short HTTP roundtrips do not prove screen presentation. This
increment retains exact launch/page/player/source-selection identity and raw observations;
immutable media correspondence, calibrated presentation error and synchronized start remain
separate required work. Source-selection IDs must not be relabelled imported source identities.

A bounded HTTP dispatcher handles parsing and response writes without running inference.
The existing Preview remains a single owned worker with bounded jobs. Player control and
native guard I/O have separate owners and deadlines. Every queued job/result retains its
original player scope; replacement and late pagehide cannot mutate a newer player. Interrupted
operations retain original request/outcome recovery, with no automatic replacement request.

Binding uses normal current authorization, exact acknowledged play/native configuration and
explicit monitoring consent, then persists its historical report before native installation.
Fast challenge/observation/release uses only the exact boot-local guard, captured authenticated
principal and local owner proof. It executes before database-dependent Engine dispatch. Existing
public grant, revision, configuration and session retirement must invalidate guards before their
mutations return. Fast cached authority does not promise immediate detection of arbitrary
out-of-band database edits.

The existing controller monitor enforces independent heartbeat and media-progress deadlines;
a heartbeat cannot renew controller or Handy timing authority. Repeated frame/media values
cannot indefinitely renew progress. Expiry and explicit stop retire through the existing stop
owner, retaining physical uncertainty. Old nonce/sequence/player traffic is inert. A pending
bind is not yet a confirmed native guard; late bind success after local retirement is released.

The [feature guide](../../docs/PULSAR_BROWSER_PLAYER_CONTROL.md) records composed software and
isolated native HTTP verification. This does not establish rendered browser behavior, physical
stopping, useful inference or the 200 ms target.
The complete goal still requires source/presentation mapping, generated HSP point streaming,
and real model/media/platform/device/human/release evidence.
