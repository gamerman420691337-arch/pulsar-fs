# ADR B5-045 — Guarded native play submission and shared execution ownership

Expose original play/status/unclaimed-cancel operations through Engine authority. Reserve the
original request, atomically prepare its journal operation/effect, and commit an exact main
witness before queueing. Recovery may repair missing metadata but never recreate native work.
Validate immutable witness subjects at startup, on queries and at final dispatch authority.

Extend the retained-stop owner with play using the same runtime, transport and dispatcher.
Share the original DB mutex through a standalone authorizer, preserving dependency guards
through the dispatch claim and releasing them before HTTP. Retain stop progression independently
of play metadata failure. Keep Playing custody distinct from original Armed admission and
preserve original clock age across controller renewal. Workers must not retain Engine itself.

Publish journal version 6 despite unchanged table layout: earlier readers do not enforce the
new play-route exclusion on setup/synchronization claims. Acknowledged transport outcomes still
reserve unresolved physical effects. Only exact unclaimed cancellation releases that reservation.
Keep generic desktop playback disabled and advertise scoped guarded protocol operations separately.
This decision does not close browser synchronization, continuous clock renewal, physical-device
qualification, emergency stopping, daemon shutdown or attempted-effect reconciliation.
