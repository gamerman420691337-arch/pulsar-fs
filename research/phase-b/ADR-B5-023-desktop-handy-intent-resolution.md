# ADR B5-023 — Resolve original registration identities before clearing uncertainty

A desktop save can commit before its reply is lost. Matching current configuration
does not prove which request applied, and a later refusal does not settle an earlier
unknown attempt. Preserve the exact intent privately before dispatch and add an
owner-authorized Engine resolution operation.

Resolution reconstructs and checks the original request fingerprint in the same
authenticated session. Under one Engine transaction it either returns the original
saved receipt or records a withdrawn result under the original identity, together
with its own resolution receipt. That reservation prevents a delayed identical save
from applying. A mismatched original intent refuses. Historical acknowledgment is
separate from current catalog state; resolution never undoes a saved configuration.

The desktop captures save/disable inputs and retry IDs, restores them inertly, and
allows local dismissal only after a recorded applied/withdrawn outcome. Resolution
IDs are retained before dispatch too. Owner proof is captured once, hashed for local
binding and passed unchanged to the shared helper; no bearer values or online consent
are journalled. Changed selection/draft state clears inspection consent and observations.

The per-request withdrawal response is a fail-closed barrier for preceding readers:
an older reader cannot decode it and therefore cannot replay its original mutation.
Registration metadata remains inert; no physical authority or project-format meaning
changes. See [contract and evidence](../../docs/PULSAR_DESKTOP_HANDY_REGISTRATIONS.md).
