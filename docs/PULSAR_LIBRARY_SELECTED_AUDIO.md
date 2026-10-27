# Selected audio axes in retained local batches

Retained audio intake through CLI and desktop now preserves an existing adjacent
`<media stem>.funscript` by selecting only sway, surge, roll, pitch and yaw for
publication. The output is the usual new
`<media filename>.pulsar-audio-bundle` directory, containing those five neutral
files and its bundle receipt. No duplicate stroke file is published in that
bundle. With no adjacent stroke, the original six-file output remains unchanged.

Presence protects an adjacent regular file even when its content is empty or
invalid. A symlink, directory or metadata error at that path refuses intake.
No content validity or source correspondence follows from presence. Existing
bundle directories remain excluded from this intake; filling a previously
partial bundle is a separate remaining requirement.

`LibraryBatchInput.publication_axes` is an optional explicit audio subset in
canonical neutral-axis order. Omission preserves the original wire and six-axis
behavior. A subset contains one through five distinct axes. Visual recipes retain
their stroke-only contract and reject input-level audio subsets.

The Engine captures this choice with the original immutable source and recipe.
Audio synthesis still produces a full six-axis candidate; commit checks the
generation family separately from the selected publication axes. The user-facing
batch output selection does not change that internal candidate or constitute
useful-motion qualification. Publication requests and final receipts contain
exactly the selected output axes.

Generation and publication reports use schema 5 for selected audio, separately
from visual schema 3 and pack schema 4. Cancellation retains schema 5 and the
captured axes. Migration carries the unfinished entries' immutable output choices
to its successor. Desktop acknowledgement checks and publication-journal
reconciliation reject changed axes and recipe-family reinterpretation. Restart
does not rescan companions or expand a recorded selection.

New selected-audio records raise the outer Engine reader floor to 33. Migration
admission preserves a higher existing floor. Earlier audio, visual, pack and
migration records retain their prior semantics. An older reader must refuse new
storage rather than reinterpret these inputs.

Publication uses the existing selected-axis neutral-bundle publisher: private
staging, exact project/revision and request identities, then one synced
no-replace directory publication. An existing target is preserved. Completed
receipts can be inspected/repeated without replacement jobs or exports. No
partial merge or overwrite of an existing bundle is implemented here.

The scoped native campaign uses authored two-level PCM and compares every emitted
timestamp/position to its mathematical envelope mapping. It checks CLI restart,
the actual desktop controller, selected/default outputs, exact completed replay,
reader refusal and cancellation. This is software evidence only; rendered GUI,
real-media quality, annotations/models, Handy hardware, non-native platforms and
human/release acceptance remain open. Full roadmap remains PARTIAL.

See [ADR B3-180](../research/phase-b/ADR-B3-180-library-selected-audio.md) and
[checkpoint](evidence/roadmap-2026-10-12/library-selected-audio/checkpoint.json).

Validated subject: Engine/Clients/Protocol suites pass 889/402/287 with 70/10/0
ignored. Both strict lint configurations, Windows source check (existing
warnings), native/controller builds, formatting, architecture and 36 guards pass.
Final Client-only reconciliation/display changes are separately recorded against
the unchanged Engine/Protocol subjects. Native CLI and desktop controller publish
22 oracle-matching files with 101 original-clock samples each, preserve existing
bytes, and create no replacement jobs or exports on completed replay. Reader 32
refuses the new database without changing its main-file bytes. Two owned Engines
are joined; cancellation/release preserves recorded subsets and companion bytes.

The later [partial-bundle workflow](PULSAR_LIBRARY_PARTIAL_BUNDLES.md) extends intake to existing regular audio directories and supplies separate per-channel recovery evidence. The exclusions and evidence above describe this earlier checkpoint.
