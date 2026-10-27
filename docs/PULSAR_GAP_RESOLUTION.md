# Explicit authored gap-resolution proposals

2026-10-13. Core recipe, CLI proposal and scoped native commit/export implemented.
Browser gap controls and model/device/human qualification remain open.

## Use

Commit the reviewed recording candidate separately first, retaining its original
gaps and provenance. Select exact gaps from the committed project program; the
selection file is a JSON array of project-nanosecond intervals, for example:

```json
[{"start":0,"end":100000000}]
```

Use the current development executable rather than the older installed shortcut:

```text
/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar gap-candidate PROJECT /absolute/selected-gaps.json --revision REVISION --axis stroke --linear-edge-hold
```

The required flag chooses authored linear interior bridges and nearest-value
holds at leading/trailing gaps. It does not choose a trained model or assert the
missing motion was observed. The command creates an uncommitted proposal through
the ordinary values upload broker. Review and commit it separately, then export:

```text
pulsar review PROJECT --candidate FILLED-CANDIDATE
pulsar commit-candidate PROJECT FILLED-CANDIDATE --revision REVISION --request-id ORIGINAL-COMMIT-ID
pulsar export-axis PROJECT /new/resolved.funscript --axis stroke
```

A failed upload follows existing values-upload recovery semantics; this command
adds no durable client intent or new exact-retry guarantee. Inspect retained
outcomes before blindly repeating a failed proposal command. Exact-ID commit and
normal export recovery remain separate operations.

## Recipe and evidence semantics

Select 1..4096 ordered, distinct, exact existing gaps on one axis. Partial-range,
missing-axis, absent-anchor and protected-stencil selections refuse. There is no
implicit zero position or automatic unlocking. Original knots, their evidence,
other axes and unselected gaps remain unchanged. Added knots are Synthesized.
Leading fill starts at the declared gap start; trailing fill reaches its final
nanosecond. Interior bridges add a millisecond-grid knot when possible; very tiny
interior gaps can use their original endpoints without inserting a colliding knot.

The shared values-only reconciler records authored and inherited span ancestry.
Knot evidence is not interval evidence: original Inferred knots can bound an
explicitly authored interpolation. Original missing-source warnings remain
historical facts, and no review disposition is manufactured. Neutral export still
refuses remaining gaps, negative time or millisecond quantization collisions;
this recipe is not automatic resampling or physical speed/synchronization admission.

The Client checks exact current revision and a matching protection snapshot before
upload, and validates returned scope, program/evidence and absence of a generation
job. Engine authority, revision, protected commit and immutable edit receipts are
shared with normal editing. No Engine/Protocol schema or authority shortcut was
added. Gap selection uses bounded lookup and linear removal over the source gaps,
avoiding a source-gap-by-selection Cartesian scan.

## Evidence

Core/Client suites pass 184/414 with 0/10 ignored. Both strict Clippy runs, Windows
source checking, native/controller builds, formatting, architecture and 36 guards
pass. Engine/Protocol source identities are unchanged; no fresh suites are claimed.
The initial test-fixture Axis::Twist compile error is retained; the actual Axis::Yaw
fixture and final checks pass.

A 50-second native authored-scalar campaign records 492 estimates across four
captures and three handoffs. The original committed recording retains five gaps
and refuses export. Explicit selection creates a second candidate with all 492
original knots unchanged and five Synthesized knots. The master remains unchanged
until a separate fixture commit to revision two. Shared immutable edit receipts
mark exactly the five original gaps authored and preserve the surrounding inherited
coverage. Independent value/receipt/export checks pass. The resulting neutral
funscript has 497 strictly ordered actions from 0 to 60000 ms. The retained scalar
fixture rounds all positions to 38 percent; it is a diagnostic constant program,
not useful automatic motion inference or a physical playback demonstration.

Final native counts are one project, zero generation jobs, two candidates and one
export. All owned processes are joined and staging custody is empty. Useful
real-media/model accuracy, broader fault/recovery/scale/platform cases, browser
controls, Handy timing and human/release acceptance remain open. Full roadmap PARTIAL.

See [ADR B3-192](../research/phase-b/ADR-B3-192-gap-resolution.md) and
[checkpoint](evidence/roadmap-2026-10-13/gap-resolution/checkpoint.json).
