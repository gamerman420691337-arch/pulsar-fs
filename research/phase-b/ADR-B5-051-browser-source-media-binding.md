# ADR B5-051 — Imported browser media with declared source correspondence

Bind Browser live to exact imported bytes and one active source placement, while
keeping the browser-to-source clock explicitly declared. A file-picker label or
player selection epoch cannot establish imported-source identity. Conversely,
verified bytes alone do not establish how a browser maps their timestamps or when
their frames are displayed. This decision preserves both distinctions on the path
to synchronized generation.

The Engine catalogs current available media placements and admits an original
preparation request with project/revision, source hash/length, exact placement,
declared anchors and finite lease. It retains a regular descriptor and source
hold, verifies the whole file, then authenticates affected blocks for each read.
An open descriptor alone is insufficient against same-inode modification. Reads
recheck current native authority and expiry; source bytes remain read-only. The
current descriptor implementation is Unix-only and bounded to 64 GiB, subject to
stricter configured resource limits.

Preparation and playback have separate lifetimes. A prepared selection may load
metadata and seek before explicit Play; its URL survives pause/seek while the
player generation and capture retire. Replacement, release, expiry and native
authority changes retire the source. Historical preparation reports retain facts,
never recover a descriptor, revive a lease or rerun a preparation. This avoids
turning retry or startup reconstruction into new read authority.

The browser uses a same-origin opaque media URL backed by a separately bounded
Range lane. Native code retains the full selection and rational values; browser
messages name a cached catalog generation and entry instead of reconstructing
native identity from JavaScript numbers. Source delivery therefore does not share
the inference worker or player-control owner. That separation still requires
runtime evidence for responsiveness under stalled reads.

`DeclaredSourceClock` maps browser presentation `B` to native elapsed time as
`E = Ea + (B - Ba)`; the active placement maps `E` to project time. Exact checked
rationals preserve offsets, trim and leading gaps. The declaration is captured
before playback, not inferred from whichever frame arrives first. Browser-bound
capture supports forward unit-rate placement and explicitly acknowledged
nonnegative anchors. Defaults are not calibration. Nonzero native PTS, edit lists,
variable frame timing and browser decoding remain correspondence questions for
real media/browser qualification.

Capture pins the live binding and retains it in reports, continuation, recording
pages and candidate provenance. Assembly rejects differing bindings or mixed
bound/unbound pages; candidates carry `browser_source_clock_declared` for review.
This checks declared time/source consistency but does not attest the origin of
submitted RGB pixels. Local file, camera and shared-tab capture remain explicitly
unbound. An optional existing Handy stop guard may also pin the selected source
so its retirement reaches the existing stop owner; no metadata DTO grants device
authority.

The [feature guide](../../docs/PULSAR_BROWSER_SOURCE_MEDIA.md) records the concrete
workflow and current validation boundary. This increment supplies source
correspondence needed for later synchronized start, but adds no synchronized-start
operation, HSP streaming, physical stopping proof or qualification acceptance.
Scoped software, native HTTP and rendered authored-fixture validation pass. Prepared
handoff reconciliation requires the exact existing successor and superseded predecessor;
queued programmatic pause notifications cannot retire a replacement playback. Unix CLI
termination uses its normal joined cleanup path. Calibrated presentation, useful model
output and physical timing remain separate requirements, as described in
the [browser timing research](browser-player-timing-and-liveness-2026-10-22.md) and
the [preceding stop-guard decision](ADR-B5-050-browser-player-stop-guard.md).
