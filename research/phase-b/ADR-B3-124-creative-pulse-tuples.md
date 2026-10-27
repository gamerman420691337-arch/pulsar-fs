# ADR-B3-124: Core-valid creative pulse choices and channel tuples

Status: accepted software repair; broader learned intent gate FAIL; roadmap PARTIAL.

The native creative schema forced every pulse duty to 0.5 despite Core supporting other adequately sampled plateaus. The pinned worker actually changed both explicitly requested 0.25 and 0.75 duties to 0.5. It also offered unrestricted channel choices for every array item, permitting duplicate/omitted channels that the independent protocol validator would reject. The baseline learned six-axis hold and triangle already passed; the duplicate-channel issue is a structural admission limitation, not a claimed observed baseline failure.

Offer pulse duty 0.25, 0.5 and 0.75 through separate schema variants. Filter each frequency by Core's minimum of two sample intervals on both pulse plateaus. Other pattern kinds retain duty 0.5. Emit an exact tuple slot for each declared host axis, with its channel fixed and pattern choices retained. Keep single-channel item schemas bounded to one item. The pinned runtime's JSON-schema parser and grammar converter support exact tuple item schemas; independent protocol/Core decoding remains mandatory.

Exhaustive schema-choice tests cover every offered parameter combination for all six channels at low and high sampling rates. Core admits them all. Tests also prove quarter-duty variants cannot offer undersampled frequencies and preserve ordinary native schema bytes.

Paired native worker probes use the same pinned runtime/model, requests and limits. Both pulse-duty requests change from FAIL to PASS; triangle, hold, six-axis hold/triangle and malformed-scope refusal remain PASS. Unavailable-yaw intent remains FAIL. Actual owned Engine/CLI runs independently check both pulse candidates, exported amplitudes, explicit commit, Undo/Redo, exact retries and stale refusal. No device, general creative quality, default-model or release acceptance is established.

See [capability and source-bound evidence](../../docs/PULSAR_CREATIVE_PULSE_TUPLES.md).
