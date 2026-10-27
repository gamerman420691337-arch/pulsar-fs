# Own the HSP session through generation retirement and shutdown

Keep HSP runtime, native writers, post-Setup acquisition and completion metadata
under an independent Engine-owned pump with bounded admission and one sample queue.
Require explicit original Play, current native authority and joined writers before
advancing the lifecycle. Retire an exact proven native-unsent operation independently
of journal cancellation so unavailable metadata cannot block the next stopping
obligation. Preserve opaque runtime/transport identity through acquisition and
rebinding, and keep ordinary retirement distinct from retained Stop authority.
Expose authenticated read-only snapshots without creating or renewing a session;
public admission remains gated on HSP-specific qualification and a checked producer.

Read the evaluation clock and current facts under the runtime mutation guard.
An independent watchdog expiry read remains available during contention, but is
never reused after acquiring that guard. Preserve sample capture/acceptance times
and original pending Play references; do not clamp clocks or weaken regression
checks to conceal concurrent ordering.

Do not hold that runtime guard across journal reads or claim commits. Retain exact
native reservation custody during durable I/O, then release the journal guard and
reacquire the runtime for fresh final time/facts and effect validation. Preserve
all earlier deadline bounds. Post-claim retirement remains an Unknown attempt,
without a send or retry, even when the command never reached the peer.
