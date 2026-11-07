# ADR B4-005: Exact placed-source generation sampling

Status: implemented locally; model, media, platform and release qualification remain open.

## Decision

An optional immutable source-placement definition is part of generation settings. The engine resolves its ID against active membership and requires exact definition/source correspondence before admitting an attempt, including retries. The worker checks source/version/base-revision binding and records the full definition in the immutable configuration/checkpoint. Callers cannot substitute a changed anchor behind an existing ID.

The core sampling plan validates the actual monotone source PTS index, preserves its original origin/timestamps and selects stride-aligned source ordinals inside the half-open elapsed-source trim. Exact source seconds map through the placement before a single nanosecond quantization. The plan retains exact mapping and rounding remainder per sample and a separate project-order index. Source analysis stays in natural decode order; reverse playback orders its resulting observations in project time, preserving positions and evidence attribution. This does not turn source-direction tracking into physical reverse-playback qualification.

Neutral motion uses nonnegative, strictly increasing nanosecond action times. A placed trim extending into negative motion time, arithmetic overflow, absent selected frames, admission overflow or colliding quantized samples is refused explicitly. Samples are never silently dropped or clamped to make a candidate fit. Inclusive reverse source-start coverage and exclusive forward source-end coverage remain explicit at the integer boundary.

No decoded support is asserted between a declared trim edge and the first/last selected sample. Those boundary intervals become unavailable gaps and review flags. Existing unavailable-sample intervals are mapped in project order, merged without overlapping actions, and remain unavailable. Receipt schema 2 carries exact placed samples, source/decode order and the project-order plan. Ordinary generation keeps its clip-relative mapping and schema-1 numeric rounding fields.

Generation creates a candidate. It does not retime existing masters or commit over protected motion. Existing revision/authority/cancellation and candidate commit/merge/rebase boundaries continue to apply. Desktop's selected placement applies to generation; `pulsar project generate-placement PROJECT PLACEMENT` submits the same engine operation and reports its job. The CLI path currently selects the unqualified model-free recipe; configured-model generation remains available through the shared API and desktop.

## Stash boundary

Stash sibling scripts play against the original file clock. Before publication, the originating job manifest is checked: a placed mapping must preserve exact elapsed-source time (unit rate and equal exact anchors). Shifted, sped-up or reversed project-time candidates require an explicit source-time export; ordinary sibling publication refuses them. This guard avoids publishing project-clock timestamps as original-file timing. Source-time export for edited placed descendants remains an open implementation requirement.

## Evidence boundary

Core cases cover irregular/nonzero source PTS, selected stride/trim, forward/reverse order, exact subnanosecond remainder, uncovered edges and invalid/negative/colliding/overflow/admission inputs. Source-clock identity requires exact anchors and unit rate.

Native lossless RGB generation compares forward and reverse candidates: source ordinals, mapped times, positions and evidence agree after project-order reversal, with uncovered boundary gaps retained. Future placement creation revisions refuse. An authenticated engine/CLI process case rejects a forged active definition, generates a reverse placed candidate after restart and leaves the protected project's revision and neutral motion unchanged. Generic native Stash cases exercise ordinary publication; the source-clock kernel is separate from a fully qualified placed-descendant Stash export.

Remaining: explicit existing-motion retiming/review resolution; source-time Stash export for placed descendants; broader codec/timebase/platform and comparative real-media/model evidence; synchronized audio and physical Handy qualification. Local sampling/generation does not close GB-03 or the roadmap.
