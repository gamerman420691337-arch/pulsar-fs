# ADR B3-201: Preserve empty stopped scopes without fabricating motion

A stopped continuation may legitimately have no estimates. Retain its complete
actor/boot/source/profile/page facts rather than refusing populated predecessor
recordings or silently discarding the empty scope. Only actual points reach Core
assembly; unavailable coverage stays a gap. Keep all-empty, active/discarded and
foreign/mutated inputs refused. Inference continuity and admission guards remain.

See [defect, correction and evidence](../../docs/PULSAR_EMPTY_RECORDING_SCOPES.md).
