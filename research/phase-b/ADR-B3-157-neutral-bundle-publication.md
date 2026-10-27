# ADR B3-157: atomic neutral bundle publication with exact export recovery

Date: 2026-10-12. Status: selected software contract; qualification open.

Keep Engine authority for every selected-axis export. A shared authenticated client helper persists one private intent with exact project/revision, canonical destination, credential binding and six distinct named-axis/request-ID records before dispatch. Resume reuses those exact commands, requiring current authorization and refusing Engine unknown outcomes; content matching alone never substitutes for an export acknowledgement. Read-only snapshot checks require the original current revision and all six neutral master axes.

Hold and lock the private staging directory and parent, verify neutral actions against the exact master, enforce bounded regular single-link members and stable metadata/name identities, sync each file/directory and publish by one no-replace directory rename. Existing destinations are preserved. Distinguish NotPublished, OutcomeUnknown and PublishedDurabilityUnconfirmed. Retain staging on failure; do not auto-delete, rebase, generate fresh request IDs or claim unknown operations succeeded.

Expose export-bundle, export-bundle-resume and optional audio-synth --atomic-bundle. Keep individual exports as the portable existing path. Atomic publication is supported only on Linux/Android under the current filesystem contract; other platforms explicitly refuse before audio generation in atomic mode. Published directories remain private and contain six provenance-free funscripts plus a hidden recovery intent with project/revision and non-secret identity metadata. No source media/private project package is included.

See [scope and evidence](../../docs/PULSAR_NEUTRAL_BUNDLE_PUBLICATION.md). Native crash/power-loss, other platform publication and rendered desktop export controls remain separate qualification/work. Full roadmap PARTIAL.
