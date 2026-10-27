# ADR B3-191: Expose candidate intent before browser staging effects

Prepare and return an immutable recording/range/request identity before dispatching
candidate creation, then retain exact retry and read-only diagnostics separately.
A single combined action with replacement retries would hide the original request
after lost acknowledgement; separate preparation makes the identity inspectable
and keeps unconfirmed effects distinct from an absent candidate.

See [contract and evidence](../../docs/PULSAR_BROWSER_RECORDING_CANDIDATES.md).
