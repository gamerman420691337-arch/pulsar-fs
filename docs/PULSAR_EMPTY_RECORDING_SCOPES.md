# Empty stopped recording scopes remain unavailable evidence

Stopping immediately after a continuation becomes ready can leave a retained
recording scope with zero estimates. The initial rendered QA produced exactly
that state: 153 estimates in the predecessor and zero in its successor. Engine
staging refused the whole playback with `stage stopped, retained, nonempty
recordings only`; no candidate or zero-valued motion was accepted.

Engine and Protocol staging now permit complete stopped, retained empty scopes
while preserving actor, original request, boot, project/revision/source/profile,
page-order and byte/entry checks. Empty pages retain their identities in the
original source proof. They add no points. Missing edges and scope boundaries
remain unavailable intervals in the pure Core assembly. Active/discarded scopes
and an aggregate containing no estimates still refuse; no stationary zero or
synthesized action is introduced by this correction.

Protocol controls compare mixed nonempty/empty assembly against the exact original
program and retain both request IDs; all-empty and active-scope inputs refuse.
The Engine all-empty case leaves revision, sources and candidate count unchanged.
The final real CPU native campaign creates an additional continuation, starts its
recording and stops without pixels. Its zero-count page is included with four
nonempty scopes in staging; all 492 original inferred knots remain, and the full
proof retains five scopes with an empty final sample array. Explicit later gap
repair adds five synthesized knots, yielding 497 neutral actions.

The initial fake continuation test changed the inference declaration and was
correctly refused; the production continuity guard was not weakened. Final suites
pass 882 Engine and 297 Protocol tests with 70/0 ignored, strict checks and native
workflow verification. These are software/diagnostic proofs, not real-media model
quality, CUDA/platform, physical device or human/release acceptance. Full roadmap
PARTIAL. See [checkpoint](evidence/roadmap-2026-10-13/browser-export-retirement/checkpoint.json)
and [ADR B3-201](../research/phase-b/ADR-B3-201-empty-recording-scopes.md).
