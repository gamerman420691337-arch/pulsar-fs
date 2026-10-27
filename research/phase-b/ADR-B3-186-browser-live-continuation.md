# ADR B3-186: Browser playback uses prepared Engine successors without extending leases

Status: implemented Client coordination; scoped CPU native evidence only.

A browser preview that stops at each 30-second capture lease cannot provide the
intended continuous experience. Reuse the Engine's prepared continuation authority
rather than extending leases or allowing the browser to declare backend readiness.
Prepare at half-lease, retain the current worker, derive exact project phase and
feed separate admitted channels. Adopt only after Running plus the matching
predecessor's Superseded observation. Keep the browser playback identity stable.

A frame body received before a new channel handshake cannot be relabeled with a
later receipt stamp. Defer or drop it; ambiguous transport/continuation outcomes
fence the preview. Attempt Stop for both owned scopes and preserve uncertain
outcomes. Capacity and current grants remain admission boundaries, and no model
fallback or physical playback authority is added.

See [behavior, evidence and limits](../../docs/PULSAR_BROWSER_LIVE_CONTINUATION.md)
and [checkpoint](../../docs/evidence/roadmap-2026-10-13/browser-live-continuation/checkpoint.json).
