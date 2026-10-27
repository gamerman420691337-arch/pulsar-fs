# Receipt-bound playback observations

The conditional Handy coordinator now accepts play callbacks with the same exact
receipt checks and metadata recovery available for stop callbacks. This supplies
the callback side of an asynchronous dispatcher; no network transport, qualified
device registry, public playback command or physical qualification is enabled.

`observe_play_completion` checks the current play effect and its immutable receipt
correspondence, retires the play callback, and returns a `PlayCompletion`. Its
`observation` retains the original effect, receipt and outcome. Its independent
`stop` result contains the stop intent required by an unknown play outcome, or an
explicit construction error. Stop construction failure cannot discard a matched
play observation. Repeating the callback refuses; it cannot replay motion.

`observe_stop_completion` similarly retires a matched stop callback and enters
`StopUnconfirmed`. Both observation methods take an `EffectReceiptBinding`, which
contains only the journal's captured boot identity and immutable correspondence
logic. It has no database handle and performs no journal I/O. An owning dispatcher
can release its control-state lock before attempting `observation.persist(journal)`.
The binding is neither a current database-ownership check nor dispatch permission.
Persistence still checks current boot ownership, stored integrity, exact receipt
identity and terminal-outcome consistency. Transport handoff still requires fresh
Engine authority and qualification checks.

An observation starts with `journal_pending = true`. Explicit persistence records
only its captured outcome, clears that flag on success, and leaves it pending on
failure. Retrying metadata cannot issue a new play/stop, retire a newer effect,
restore a controller or clear physical uncertainty. A new engine boot fences the
old observation. Loss of an unpersisted in-memory observation leaves the durable
prepared row unresolved; restart cannot invent the missing transport outcome.

The dispatcher also exposes `historical_observation` for an Observed completion
that arrived after revocation or a newer stop made its live callback stale. It
checks the immutable effect/receipt binding and returns a pending metadata record
without accessing or changing runtime state. Rejected pre-claim attempts cannot
create history. Normal current callbacks still require `observe` and handling of
the returned stop result. The [dispatch clock checkpoint](evidence/roadmap-2026-10-22/handy-dispatch-clock/checkpoint.json)
includes an actual delayed local HTTP play response, intervening revocation, an
injected journal UPDATE failure, exact metadata retry and preservation of the newer
stop and absent controller. No physical stopping evidence is inferred.

`complete_play_with_recovery` is a synchronous convenience wrapper that observes
and attempts persistence. Existing stop completion methods retain that behavior.
`StopCompletionRecord` remains a compatibility alias for the shared
`EffectCompletionRecord`; the journal schema, encoding, capacity and outcome
vocabulary are unchanged. No observation/binding type is deserializable. A network
acknowledgment remains a transport fact and never supplies physical stopping proof.

Tests use synthetic qualification facts and actual private SQLite stores. They
cover acknowledged/unknown results, UPDATE-abort failures, exact repeated metadata
retry, receipt substitution, cross-boot refusal, reconnect, conflicting stored
outcomes and stop-counter overflow. A separate threaded case holds a SQLite writer
lock while observing play and stop, then releases the writer and persists both
records. Existing coordinator checks and the public fail-closed playback test are
run separately from physical qualification. Final commands and source identities
are in the [checkpoint](evidence/roadmap-2026-10-16/play-completion/checkpoint.json).

Remaining dispatcher work includes owned transport execution, fresh authority and
clock checks at handoff, cancellation/order/deadline behavior, emergency stopping
when storage fails, registry/qualification intake and public client integration.
Actual Handy 2 Pro firmware, transport, stopping and synchronization evidence,
six-axis behavior, model/media, platform and human/release acceptance remain open.
Full roadmap PARTIAL. See the [decision](../research/phase-b/ADR-B5-020-observed-play-completion.md)
and [existing stop recovery](PULSAR_STOP_COMPLETION_RECONCILIATION.md).


## Scoped verification

Eight new callback/reconciliation cases pass on the ordinary filesystem. The full
coordinator filter passes 40 tests with no failures or ignored cases, including the
existing stop recovery and quota tests. The real Engine handler's public physical
playback refusal test also passes. Strict Engine/all-target and workspace/all-feature
Clippy, App/Worker builds, formatting, architecture and source guards pass.

The blocked-writer case observes both play and stop on separate threads while an
independent SQLite connection retains its write transaction. Both observations arrive
before that transaction is released, with journal rows still unresolved; explicit
persistence afterward records the two original outcomes. Physical stopping remains
unconfirmed. This tests control/journal separation, not a measured device-latency gate.

The earlier synchronous-only implementation and its seven/39 passing tests are retained
as an intermediate stage, not substituted for the final eight/40 case subject. No fresh
full Engine, Client, Core or Protocol suite is claimed here. Earlier ordinary-storage
full-suite timeouts and broader contention failures remain unresolved on their recorded
subjects. No Handy, browser, media/model or human qualification was performed.
