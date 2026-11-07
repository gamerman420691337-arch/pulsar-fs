# Average segment-speed diagnostics

Pulsar can query average normalized motion slopes through the shared authenticated engine. This is a numeric editing diagnostic. It does not measure physical velocity, peak trajectory speed, acceleration, jerk, model accuracy or device stopping.

## Consumers and effects

- `pulsar doctor FILE.funscript --max-speed 450` captures the script in a retained engine project as an **uncommitted candidate**, then queries that candidate. The source file remains unchanged and the committed master remains empty. The retained project ID is printed on stderr and included in the report. Import failure can leave the diagnostic project retained. Invalid limits are rejected before project creation.
- `pulsar review PROJECT [--candidate CANDIDATE] --max-speed 450` queries the named master or same-project candidate. Without `--max-speed`, existing evidence/protection diagnostics remain unchanged and no speed scan is implied.
- Desktop committed-revision review has a maximum percent/second control, initially 450. Candidate inspection uses the same selected limit. The report displays localized numeric slopes and visited/unavailable counts. Rendered usability is not qualified by compilation or native CLI checks.

`Diagnostics.maximum_speed_percent_per_second` and `DiagnosticsReport.segment_speed` are additive optional fields. Omitted fields preserve the old contract. The diagnostic query requires authenticated project read authority, does not require an expected revision, and does not change project motion, protection, reviews or history. Standalone `doctor` additionally creates a project and imports a captured candidate under existing mutation authority; the overall standalone workflow is not effect-free.

## Measurement and bounds

For each adjacent action pair, the scan computes `abs(position_delta) * 100 / elapsed_seconds`, using checked nanosecond action times and finite floating-point positions. A slope equal to the selected limit does not violate it. Valid limits are finite and in `(0, 100000000000]` percent/second, the normalized full-range change over one nanosecond.

An edge overlapping an explicit unavailable gap is counted as unavailable and skipped; gaps never become interpolated motion. Constant segments have zero slope. All present axes are scanned without assuming physical units or six-axis reconstruction.

The engine retains at most 128 violating edges per query. Discovery of another violation stops the scan and sets `truncated`; counts then describe only the visited prefix. A nontruncated scan visits all edges. The typed report identifies axis, exact half-open action interval and numeric slope. Localized issues coexist with the existing bounded evidence/protection page. Serialization remains bounded by the control transport.

No automatic motion repair follows. The legacy `fix` adapter and jitter repair remain pending; a low average slope cannot establish a device-safe trajectory.

## Verification

[The source-bound checkpoint](evidence/roadmap-2026-10-01/segment-speed-diagnostics/validation.json) records the actual gate outcomes. Core regressions cover equality, extreme nanosecond times, invalid limits, gaps, holds and explicit page overflow. Protocol regressions cover optional legacy fields, bounds and read-only classification. Engine regressions compare unchanged master/review state and protection across repeated candidate/master queries, threshold changes, restart and undo.

The native authored-script scenario checks standalone retention without commit, original-file SHA-256, invalid-limit rejection before any project creation, exact threshold changes, explicit 128-entry page overflow, unchanged master/review reports and an actual engine restart. It uses isolated authored fixtures and contacts no real Stash, Handy, camera or private reference media. These checks establish local software behavior only; roadmap/model/device/platform/human/release qualification remains open.
