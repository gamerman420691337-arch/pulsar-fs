# ADR-B3-119: explicit axis-pattern clients and shared admission preflight

Status: software implemented; verification recorded separately; qualification open.

The existing worker accepts typed one-through-six-axis patterns, but CLI and desktop only expose single pattern text. Add a shared protocol-only builder for bounded ASCII text containing one strict pattern per nonempty line. Use existing checked PatternSpec and GenerationInput values; do not add a new wire operation or duplicate motion synthesis in clients.

Extract the existing Core family preflight from synthesis and reuse it at protocol admission. Reject mismatched durations, duplicate axes and aggregate allocation overflow before a generation job can be accepted. Preserve existing individual-pattern validation and explicit synthesized evidence.

Expose a revision-bound CLI candidate command and a desktop Generator input mode using the same checked builder. Keep review and commit separate, with existing protected editing/export/history semantics. Actual controller, immutable source/worker and independent numeric/export checks are separate from rendered desktop/human acceptance and from visual six-axis/model/media/device qualification. See [workflow and evidence](../../docs/PULSAR_AXIS_PATTERN_CANDIDATES.md).
