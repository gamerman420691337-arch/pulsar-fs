# Direct source-view authoring for retained visual library batches

The Library visual recipe editor now exposes the same explicit stereo and
projection form used by the single-source editor. Choose a declared layout,
analysis eye and flat, full equirectangular 360 or half equirectangular VR180
projection. Perspective yaw, pitch, roll and horizontal/vertical field of view
are editable for spherical projections. Disabling the declaration preserves
legacy unspecified-view semantics. Mono uses the left eye as the canonical
single image. No eye order, source projection, stereo depth or tracking target
is inferred by these controls.

Capture validates the form through the existing Core VideoView contract and
clones the complete declaration into the background intake action alongside
model/preset/stride settings. Background construction places that exact value
in the shared GenerationSettings and validates it before retained Prepare.
Changing the form later cannot replace the captured retry recipe. Invalid
numeric/half-sphere/mono-eye declarations refuse without batch dispatch. An
inline view and a view JSON path cannot be combined; JSON intake remains
available as its own existing workflow. No filesystem or profile read occurs
in the form renderer.

A declaration applies to every input in the captured batch. Use separate
batches for media with different layouts or projections. Actual source dimensions,
even stereo packing, projection aspect and decode support are validated by the
existing source/Worker path, after capture if needed; form validity does not
establish source correspondence. Ordinary companion/destination preservation,
selected-pack authority, immutable media/model capture and exact recovery retain
their existing contracts. This change adds no Engine/Protocol schema or reader
barrier. A fresh scoped visual fixture still records reader floor 30; an existing
floor 34 is not downgraded by this client change.

The owned Linux native campaign exercises CLI recipe JSON and the actual desktop
controller with authored packed video and a diagnostic input-dependent detector.
Flat stereo, full 360, top/bottom VR180 and legacy undeclared view each produce
three scripts, for 12 total. All have eight original timestamps at 125 ms
intervals, numerical range and exact CLI/desktop output parity. Flat stereo and
legacy outputs additionally match the earlier independently authored neutral
position oracle. Spherical positions have no independent oracle: projection
changes the inferred relative image motion, so parity is not accuracy evidence.
Job manifests retain the exact declared view and model identity. Completed CLI
replay after Engine restart adds no jobs or exports; original companion bytes
remain unchanged. Eight final-campaign Engines are joined.

Clients pass 406 tests with ten ignored. Both strict workspace lint configurations,
Windows source check (existing warnings), native/controller builds, formatting,
architecture and 36 source guards pass. Engine/Protocol source identities remain
unchanged from the preceding checkpoint; no fresh suites are claimed for them.
The drawn Capture regression freezes the view before background work and
preserves it in the retry recipe after unavailable IPC. Invalid/conflicting view
regressions prove editable local refusal without dispatch or filesystem effects.

Earlier harness attempts are retained: an authored brightness-varying input
produced unresolved Stroke and refused publication; the known moving-texture
fixture replaced it for the scoped consumer campaign. Its usefulness is not
qualified. A cross-session replay correctly refused ownership; a diagnostic
cleanup parser failed to record the second Engine join in that failed attempt,
but a subsequent exact executable/environment audit found no owned Engine.
The harness now always stops/joins in a nested finally. An inappropriate flat
position oracle also refused projected motion; the final spherical assertions
are explicitly restricted to timestamps/range/consumer parity. None of these
failures is erased or counted as a passing quality gate.

Rendered window usability, source-matched annotations, spherical/perception
accuracy, stereo depth, six-axis reconstruction, other OS runtime, physical Handy
and human/release acceptance remain open. Full roadmap PARTIAL.

See [ADR B3-182](../research/phase-b/ADR-B3-182-library-view-controls.md) and
[checkpoint](evidence/roadmap-2026-10-12/library-view-controls/checkpoint.json).

The later [unresolved-flow diagnosis](PULSAR_UNRESOLVED_FLOW_DIAGNOSTICS.md) captures the initial gradient input's missing accepted target/background support and preserves the publication refusal. Its improved shared diagnostic reports the exact first gap; the source-view campaign above retains its original subject and limits.
