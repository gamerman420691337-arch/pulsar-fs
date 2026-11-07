# Keep fresh timing reads independent of publication locks

Two actual CPU/localhost regressions retired a playing stream during a 40 ms
observation-copy lock hold, while its original observation, sample and clock bounds
remained valid. Publish the actual native observation and exact source pin together
as one immutable value, shared with the registry record. Fresh readers acquire no
application publication mutex; final command claims still require the registry
guard and exact native identity.

A fresh timing read and a captured start have different lifetimes. A selected
publication may supply its original bounded observation while a replacement is
being published; an already-captured start token must retire on replacement.
Prepare allocations and validation first, retire the old start token, then replace
the serialized record and publication. Reads retain binding, source, lifetime,
poison, receive-time and expiry checks. They never extend an old observation or
resample an earlier command's reference time.

This avoids adding a second permission-grace state machine to the watchdog and
clock observer for contention on descriptive copies. It does not make every writer
operation nonblocking or remove the final claim's serialization. Source retirement
and expired observations remain refusals.

Implementation is integrated; rebuilt native validation remains pending. The
[original failing run](../../docs/evidence/roadmap-2026-10-23/hsp-continuity/engine-private-ram-timing-copy-first/engine.log)
and [unchanged-binary repeat](../../docs/evidence/roadmap-2026-10-23/hsp-continuity/engine-private-ram-timing-copy-repeat/engine.log)
are retained. The replacement regression holds the real publication registry after
removal of the copy mutexes. These software checks supply no physical qualification.
