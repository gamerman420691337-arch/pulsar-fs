# ADR B5-041 — Independent synchronization custody and single-use handoff

Treat the manufacturer's clocksync GET as an effect. Its request reservation, immutable intent,
cross-store witness and sole claim commit before transport. Keep synchronization custody in the
device journal independently of controller lifetime and of the already reserved setup parent.
This preserves the original setup while preventing expiry or restart from releasing an unknown
synchronization. Retire old controller/timing authority before handoff, and never replay the GET.

Only unclaimed preparation is locally cancellable. A bounded acknowledgment releases the
synchronization reservation but establishes no timing accuracy or restored admission. Unknown
and missing completions remain reserved pending a defined reconciliation mechanism. Version 4
fences older journal readers; linked missing history refuses startup before recreation.
