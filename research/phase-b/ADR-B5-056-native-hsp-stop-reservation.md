# Share native HSP Stop custody across durable and emergency dispatch

Reserve the exact runtime operation before starting either worker, and settle
that reservation only after the worker joins. A durable claim and an emergency
Stop consume the same native slot, so metadata failure cannot authorize a second
send. Permit only an already-issued Stop to proceed without a confirmed journal
claim, retaining native ownership and device/session/mode/stream preflight and
refusing an observed replacement owner. Keep the emergency outcome as separate
retrospective evidence; do not manufacture durable history or physical stopping
proof. Continuous ownership, ordinary-slot retirement during metadata outage and
retrospective persistence remain required integration work.
