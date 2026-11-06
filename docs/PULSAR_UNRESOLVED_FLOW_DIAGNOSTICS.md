# Unresolved relative-flow diagnosis and publication diagnostics

The first native source-view campaign used authored packed video with pixels
`(3*x + 5*y + 7*frame) modulo 256`. Generation completed, but ordinary Stroke
publication refused. The separate known moving-texture campaign passed. The
new native loop reproduces the refusal twice with stereo and once with mono,
and captures the actual digest-bound Worker receipts and candidate review flags.

All later gradient frames contain one observed detector box and the selected
track, but zero accepted target and background flow points. Their image
translations and relative translation are null; evidence is explicitly
unavailable from 125000000 through 875000001 ns in project time. The candidate
retains a synthesized initial anchor and the unresolved gap, with
`unsupported_relative_flow` and `missing_motion_evidence` reviews. These are
uncalibrated review facts, not physical-motion or quality labels.

The gradient lacks unique two-dimensional correspondence. At an interior 5×5
patch, distinct offsets `(1,-2)` and `(-4,1)` both exactly cancel the next frame's
brightness shift: `3*dx + 5*dy + 7 = 0`. The retained independent patch proof
checks both zero-error matches. Core flow rejects insufficient texture and tied
matches; individual point rejection codes were not instrumented, so the campaign
does not attribute every rejection to a particular one of those checks. The
Worker receipts establish the consequential fact: no accepted target/background
support. The textured control has 64 target and 16 background accepted points
at the first relative estimate, both translating vertically by one pixel; its
relative estimate is zero and its neutral script publishes successfully.

This is not a stereo renderer or missing decode failure. Removing the declared
stereo view retains the refusal, and the same detector, threshold, analysis size,
preset and publication path work for the textured input. Publication does not
silently discard the partial candidate: ordinary funscript lacks gap semantics,
so the existing shared master checks refuse unresolved evidence. Generation
attempted a best-effort proposal and retains its unavailable intervals; no
synthetic physical motion or confidence is substituted to force publication.

The software change makes that refusal actionable. The shared neutral-bundle
preflight now reports the selected axis, unresolved interval count, exact first
project-time gap and the need to inspect retained review flags or explicitly edit
the gaps. Empty tracks have a separate axis-specific error. The diagnostic is
bounded by the fixed axis vocabulary and numeric fields; it does not enumerate
an unbounded track or change the retained intent, authorization, no-replacement
publication, gap handling, generation algorithm, schema or reader admission.
CLI workflows and the shared desktop controller receive the same detail.

Final native validation reproduces all three refusals with the exact gap diagnostic,
publishes the textured control, and repeats the first refused intent after Engine
restart. Jobs/candidates/exports remain 4/4/1; no replacement work or ordinary
output is created for the unresolved input, and the private journal bytes are
unchanged by exact failed retry. Two final-campaign Engines are joined. Earlier
reproduction and receipt-path discovery attempts are retained; their owned Engines
were joined, and fixture credentials remained in private RAM. Receipt files are
named `receipt.json` by the broker, not the Worker's logical `lineage.json` name.

Clients pass 406 tests with ten ignored. Both strict lint configurations, Windows
source check (existing warnings), native/controller builds, formatting,
architecture and 36 source guards pass. Tracking/Core/Engine/Protocol source
identities are unchanged, with no fresh suites claimed for them. Authored fixture
and diagnostic success does not establish useful model/media accuracy, calibrated
review recall, motion recovery from all decoded inputs, or real client/device
failure behavior. The unresolved source remains a failed publication case.

Rendered desktop checks were not run: the agent-workspace-linux skill was read,
but its MCP capabilities were unavailable in this session. No host desktop was
operated, no workspace was started, and no GUI qualification is inferred from
these native CLI/controller checks. Other software, model/media/platform/Handy,
human and release obligations keep the full roadmap PARTIAL.

See [ADR B3-183](../research/phase-b/ADR-B3-183-unresolved-flow-diagnostics.md)
and [checkpoint](evidence/roadmap-2026-10-12/generation-unresolved-diagnosis/checkpoint.json).
