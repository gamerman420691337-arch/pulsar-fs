# ADR B5-032 — Private route namespaces and actor-scoped setup recovery

Recover setup history by the original actor/request identity without requiring its lost
receipt ID, live connection or source files. The public status command requires the host-local
owner capability and current project Read grant; it cannot return another actor's history or
construct a dispatch claim. Historical acknowledgment remains distinct from loaded authority.

Derive route identity with domain-separated HMAC-SHA256 over the exact captured connection
key, keyed by the existing private Handy owner capability. Do not persist an unkeyed digest
of a potentially short connection key. Pin the owner-key namespace in new setup records.
Changed namespaces and legacy namespace-less active history block new preparations; owner-key
rotation cannot discard unresolved routes. Logical-device reservations also survive connection
key changes. No physical equivalence across different keys and different registrations is inferred.

The optional metadata field preserves legacy canonical records and permits their history and
unclaimed cancellation. New preparation requires explicit namespaced identity. Preceding
readers reject the extended record through their read-only preflight. Native authority checks,
setup sending, loaded-program association and attempted-effect reconciliation remain separate
integration work.
