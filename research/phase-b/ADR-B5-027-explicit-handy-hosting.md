# ADR B5-027 — Journal exact disclosure before temporary hosting

Require explicit public-hosting consent for an Engine-verified preparation receipt,
current master revision and exact file hash. Freeze the bounded file bytes and commit an
unknown-outcome receipt before network I/O. Replay inspects that original identity instead
of resending content; an old Engine epoch cannot activate previously unclaimed consent.
A successful POST is insufficient: download from a constrained manufacturer path and
compare every CSV row before recording verified content.

Persist only observations and content identities. Keep the location in the native Engine,
with live availability derived from current ownership/revision and explicit retirement.
Restart restores no location or send authority. Forgetting is local retirement, not server
deletion or remote cancellation. Its durable marker prevents a late callback from restoring
use. Completion acknowledgment failure preserves uncertainty rather than converting a retry
into a second disclosure. Grant/revision changes cannot install a usable late result.

Use the existing per-session request ledger for reservation, observation and retirement,
without project-schema changes or an executable recovery queue. Both owner capability and
Read/Export grants remain mandatory. Hosting introduces no device credentials, HSSP setup,
clock calibration, physical motion or qualification. A later setup path must freshly verify
its native locator, source bytes, session and qualified admission rather than treating a
historical hosting receipt as authority.
