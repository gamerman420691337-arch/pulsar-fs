# ADR B3-162: authenticated observational bundle inspection

Date: 2026-10-12. Status: implemented software contract; qualification open.

Expose retained neutral bundle inspection through the existing credential-bound intent and exclusive directory ownership. Reuse exact master/file comparison, with missing members reported rather than treated as export success. Keep publication verification strict. Present files must retain private identity/content and the master revision must remain current across inspection. Refuse unexpected entries and unsafe recovery state.

Report captured request identities and local presence independently from Engine outcomes. Never infer acknowledged exports, authorize retry or mutate the filesystem from matching bytes. Resume remains the separate exact-intent path with its existing unknown-outcome refusal. See [scope and evidence](../../docs/PULSAR_NEUTRAL_BUNDLE_INSPECTION.md). Full roadmap PARTIAL.
