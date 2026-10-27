# ADR B2-008: annotated receipt metadata and model correspondence

Status: implemented locally; public and portable annotated workflows remain guarded.

## Context

Hash-valid worker output alone does not establish correspondence to an immutable annotated request. The new worker receipt contains a source view plan, transform and model training-intake identity that must be checked before candidate publication. Historical requests contain paths that must never become active resolvers during portable import.

## Decision

Use a shared bounded reader-based verifier. Require exact request/result identities, canonical settings/configuration digest, native planner replay, exact ordered dependencies and returned review data. Validate model recipe/intake metadata against content-verified model bytes through the same reader, hashing before and after inspection. Keep legacy whole-frame admission behavior unchanged.

Stream irrelevant analysis arrays; bound receipt bytes and retained arrays. Keep reader provenance and memory admission with the caller. The current Engine wrapper opens only its current declared model/receipt. Future portable callers must use retained object readers and an explicit version barrier; no historical request path is opened by the shared verifier.

## Evidence and remaining obligations

[Receipt correspondence](../../docs/PULSAR_ANNOTATED_GENERATION_RECEIPTS.md) records mutation/bounds tests and actual authored native receipt/model checks, including inert archived-path reader admission. This does not qualify natural-media accuracy or establish schema-3 source clocks. Portable format/read-only import/export closure, public Engine generation, client controls and broader roadmap qualification remain required before removing admission guards.
