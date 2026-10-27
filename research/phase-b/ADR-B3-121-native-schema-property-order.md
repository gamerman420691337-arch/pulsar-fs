# ADR-B3-121: native grammar choices before their parameters

Status: accepted software repair; broader creative candidate quality FAIL; roadmap incomplete.

Pulsar built native JSON schemas using alphabetically sorted object properties. The pinned llama.cpp runtime preserves those properties when constructing its grammar. Creative pattern objects consequently chose amplitude before pattern kind, so a selected nonzero amplitude eliminated hold regardless of subsequent user intent.

Serialize schema property maps in the host-declared `required` order, recursively, before any remaining optional properties. Preserve every parsed schema value. Refuse malformed required metadata rather than emitting duplicate keys. Keep this serializer private to the Linux worker composition; protocol decoding, default schema bytes, numeric palettes, bounds and effect authorization remain unchanged.

The controlled alphabetical/required-order comparison uses identical parsed schemas and the same three requests/model/runtime/settings. Triangle passes in both; hold passes only with pattern-first order; unavailable yaw fails in both. The actual rebuilt confined Pulsar worker reproduces triangle and hold PASS, unavailable-yaw FAIL and malformed-scope refusal PASS. This repairs an actual worker behavior without converting the remaining semantic failure into acceptance. Five new tests and the full default Engine suite pass within their stated scope.

See [current capability and source-bound evidence](../../docs/PULSAR_CREATIVE_PATTERN_PROPOSALS.md). Creative application approval, CLI/desktop wiring, model selection, broader prompt/media/device/platform and human qualification remain open.
