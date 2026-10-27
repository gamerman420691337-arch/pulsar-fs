# Engine-owned live recording candidates

2026-10-13. CLI/Engine staging and scoped native restart/commit evidence implemented.
Browser candidate controls, useful model quality and physical playback remain open.

## Use

Use the current development executable; the installed shortcut still targets an
older checkout. Inspect each retained capture with `live-capture recording-page`
to obtain its Engine boot, capture epoch and original recording request ID. Keep
an explicit stage request ID before dispatch, so a lost reply can be reconciled
by repeating the same command and options.

```text
/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar live-capture recording-stage PROJECT REVISION /absolute/stage-options.json --request-id ORIGINAL-STAGE-ID
```

Options name original records rather than uploading downloaded JSON:

```json
{
  "engine_epoch": "ENGINE-BOOT-FROM-STATUS",
  "recordings": [
    {"epoch": "CAPTURE-EPOCH", "recording_request_id": "ORIGINAL-RECORDING-ID"}
  ],
  "range": {"start": 0, "end": 60000000000},
  "maximum_interpolation_ns": 200000000
}
```

The range is explicit project nanoseconds, not an assumed wall-clock duration.
The interpolation limit is an explicit policy, not measured model accuracy.
Records must be stopped, retained, nonempty and listed in increasing project-time
order. The [assembly contract](PULSAR_LIVE_RECORDING_ASSEMBLY.md) retains missing
coverage and never sorts, clips or bridges capture boundaries.

Inspect the returned proposal and shared review diagnostics before deciding on
commit. Staging leaves the master unchanged; commit and export remain separate:

```text
pulsar review PROJECT --candidate CANDIDATE
pulsar commit-candidate PROJECT CANDIDATE --revision REVISION --request-id ORIGINAL-COMMIT-ID
pulsar export-axis PROJECT /new/output.funscript --axis stroke
```

Unresolved gaps continue to refuse standard funscript export even after commit.
Review flags or commitments do not establish physical qualification. Explicit
coverage resolution and browser review/commit/export controls remain follow-up work.

## Authority and durability

The Engine obtains pages from its original actor-owned live registry. Project,
admitted revision, boot and recording-intent IDs must match. Read, ImportSource,
Generate and Edit grants are required; another actor cannot stage the original
recording merely by receiving those project grants. Bounds remain 64 scopes,
256 pages and 4096 aggregate samples. Preparation holds shared validation memory
and the artifact guard; current authority/revision are checked again in the
publishing transaction.

Full original pages, clocks, profile/attempt/pixel bindings, stage intent and
provisional status are captured in a content-addressed recording source, bounded
to 40 MiB. The candidate references that source through shared durable lineage.
Source purpose is `live_recording`, distinct from media, synthetic input or a
funscript, and the desktop marks it non-previewable. This expands the source-kind
wire vocabulary; older readers may refuse the unfamiliar kind. No database column
migration or implicit relabeling of existing sources occurs.

Actions stay Inferred, gaps stay unavailable, and review includes unqualified
recording, missing coverage and CUDA provisional warnings where applicable.
Candidate creation and its original request replay are committed atomically through
the existing transaction. Exact replay survives Engine restart. Fresh staging after
restart cannot reconstruct the vanished live registry from a downloaded JSON file.
Existing portable source storage preserves historical bytes/kind; an actual
live-recording package clone roundtrip is not yet specifically qualified.

## Evidence and limits

Protocol and Client suites pass 296/411 with 0/10 ignored. The complete Engine
library suite passes 881 with 70 ignored; a final separate log restores full
suite evidence after the native harness reused its initial log name. Both strict
lints, Windows source checking, native/controller builds, formatting, architecture
and 36 guards pass. Initial desktop exhaustive-match compile failure is retained.

A 50-second native HTTP/CPU campaign records 491 accepted estimates across four
captures and three handoffs. One candidate retains all 491 points and five gaps;
independent rational arithmetic matches every time, value, evidence label and gap.
The master stays at revision zero until explicit fixture commit. Exact stage
replay after an actual Engine restart returns the same candidate; a fresh stage
refuses missing original capture state. Commit preserves the proposal and gaps;
standard export refuses with no output file or export row. No review disposition
or human acceptance was manufactured. The final ledger contains one project,
zero generation jobs, one candidate and zero exports. Staging custody is empty;
the browser server and both Engine processes are joined.

This uses an authored diagnostic scalar model, not useful motion-model inference.
Browser rendered controls, other platforms, interruptions beyond the named cases,
CUDA candidate campaigns, useful real-media accuracy, Handy synchronization and
human/release qualification remain open. Full roadmap PARTIAL.

See [checkpoint](evidence/roadmap-2026-10-13/live-recording-candidate/checkpoint.json)
and [ADR B3-190](../research/phase-b/ADR-B3-190-live-recording-candidate.md).
