# ADR B5-024 — Bind connection observations to one Engine boot and credential snapshot

Saved credential-variable names are inert configuration. An online inspection and a
later independent environment reload do not establish the same native route. Add an
owner-authorized connection lifecycle that captures the registration and credential
values once, reserves its logical device and connection-key route, and retains the
snapshot in Engine memory.

An opening metadata receipt commits before I/O. It supplies correspondence, never
qualification or physical authority. Replay consults the same boot's registry and cannot
perform another GET; an absent entry is retired. Every request carries the captured
Engine epoch, preventing delayed consent from opening a new session after restart.
Registration mutations and open completion share DB-to-registry lock order. Invalidated
openings retain their route until completion, and their callbacks cannot restore it.

Closing only retires observation ownership. No physical dispatcher is attached in this
change. Integrating one requires joining its owned job and preserving its stop obligations;
this close path must not be reused as evidence of physical stopping. The public playback
gate remains closed. Connection-key equality excludes host aliases; it is not a claim of
universal physical identity or exclusivity against other controllers.

No project schema or physical recovery meaning changes. New receipt variants preserve
request identity for older readers by failing to decode, not by supplying defaults.
Restart restores neither snapshots nor observations. Inspectors and conditional HSSP
transport share native credential validation and sensitive-header handling. No bearer
values or persisted credential fingerprints are introduced.

See [contract](../../docs/PULSAR_HANDY_CONNECTION_SESSIONS.md) and the linked scoped
validation packet. Hardware, model, real-media, platform and human acceptance gates remain
independent of these connection lifecycle checks.
