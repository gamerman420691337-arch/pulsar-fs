# ADR B5-021 — Own the HTTP job and consume one dispatch claim

Prepared effects and durable receipts establish intent, but a cloned effect could
otherwise be handed to two asynchronous transports before either callback retires
it. Consume a coordinator claim once per play/stop sequence, under current runtime
ownership, after receipt/binding/authority and timing checks. A failed or uncertain
transport never reopens that claim. Claims are ephemeral and cannot be restored as
authority from journal history after restart.

Use one owned worker and no queue per dispatcher. Return rejected submissions with
their original token. Acquire fresh Engine facts outside the coordinator lock, then
claim without waiting on that lock; release it before network I/O. Preserve the
original encoded request and recheck its original age/clock budget after preparation.
An explicit absolute local deadline bounds the request independently of authority
lookup. Fresh authorizers remain responsible for bounded, cancellation-aware lookup.

The HTTP adapter has a fixed production origin, captured native credentials, sensitive
headers, bounded response decoding, exact loaded-program/session/stream binding and
no redirects, proxies or automatic retries. Poll/shutdown return a completion for the
existing receipt-bound observation path. Drop joins but cannot invent a missing
observation or confirm stopping; normal device ownership must handle shutdown results.

This implements conditional HSSP transport, not public playback or qualification.
Registry ownership, qualification intake, script hosting/setup and consent, controller
teardown/emergency stopping, remote ordering and physical response remain open.
See [contract and evidence](../../docs/PULSAR_HANDY_DISPATCH.md).
