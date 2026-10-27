# ADR B3-177 — stale pack refusal before local batch generation intent

Status: implemented software checkpoint; qualification remains open.

Already stale selected-pack choices now refuse before the Engine records a new local-library generation effect. The original recipe stays immutable, and selection changes after this check still meet the final transactional generation fence. Existing pending request identities are preserved. Native deactivation/reselection/restart, zero added generation effects, completed cancellation and five-membership release pass; ordinary CLI/desktop publication still yields four scripts matching the diagnostic fixture. Engine tests pass 884 with 70 ignored; Clients pass 392 with ten ignored. Both strict lint configurations, Windows source check (with existing warnings), native/helper builds, formatting, architecture and 36 source guards pass. Reader floor remains 31. Full stopped-batch migration, unknown-effect disposition and model/media/platform/Handy/human/release qualification remain open; roadmap PARTIAL.

Check the current selection before creating a new generation effect, while retaining the final generation fence and exact existing pending request. An absent positive receipt is insufficient evidence to supersede a request. This bounded change prevents an already stale choice from entering that unresolved state; it does not settle races or migrate old batches.

See [contract](../../docs/PULSAR_LIBRARY_PACK_PREFLIGHT.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-12/library-pack-preflight/checkpoint.json).
