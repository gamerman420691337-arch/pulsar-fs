# Prepare a reference set

The Linux development app has a **Reference sets** tab for pairing videos with
supplied scripts, attaching static target views and declaring timing offsets.
The same file checks serve the desktop and CLI. Preparing a set does not train a
model, establish permission, verify human alignment or qualify motion quality.

## Desktop

1. Create or open a project. To draw a target view, import the video, declare its
   eye/projection in **Generator**, enable **Draw target view in preview**, and
   draw on the paused frame in **Studio**. A previously saved target view also works.
2. Open **Reference sets**. Supply the permitted `Live-Reference` folder and an
   explicit authorization record. Sterile evaluation references have a separate
   intake and are not converted into training entries here.
3. Expand **Add source / script pair**. Enter a unique entry ID, source-group
   declaration, video path and reference-script path. Paths may be root-relative
   or absolute within that root. Source and reference files must be regular files;
   symlink components inside the root are refused.
4. Choose **Attach current target view**, or supply a saved view path. Saved view
   files may live outside the media folder. Their source hash must match the video.
   Click **Check and add pair**. Large file reads run outside the UI frame.
5. Select a pair and click **Open source in preview**. This imports its video into
   the open project if that content is not already present and selects its declared
   eye/projection. This panel requests ordinary decoded frames without inference.
6. Adjust **Script offset** while comparing the video with the read-only curve.
   The comparison uses the actual accepted frame's elapsed source time, not the
   moving playhead or encoded PTS origin. Source/view mismatches hide the comparison.
7. Add up to 64 distinct sources. Enter a new export folder and click **Export new
   training set**. Existing destinations are never replaced. **Open saved reference
   set** rechecks all inputs before replacing the panel's current unsaved set.

The bottom reference plot cannot edit the project master or reference script.
Clicking it seeks an ordinary source preview; placement previews can be compared
using their actual source time but do not use source-time clicks as project seeks.
The view is a fixed rectangle, not a target track or anatomical annotation.

## Clock and curve contract

`script time = elapsed source time + reference_offset_ms / 1000`.

A positive offset means the script is already that far along at source elapsed
zero. Offsets are signed integer milliseconds within ±86,400,000. Reference actions
must have strictly increasing integer timestamps in 0–86,400,000 ms and positions
in 0–100, with 2–250,000 actions. Exact rational interpolation preserves supplied
knots and plateaus. Both endpoints are included; times outside support report no
position. Floating-point conversion is used only for drawing and display.

The existing training reader consumes raw `at`/`pos` and ignores the funscript
`inverted` flag. This preview follows that contract, which differs from normal
project script import. It neither repairs nor sorts reference actions.

## Export and existing intake

An export contains:

- `manifest.json`: existing training schema 2, source groups, training-only uses,
  supplied authorization record and a required offset for every entry.
- `input-views.json` and `<entry-id>.view.json`: existing source-bound static-view
  declarations. Sidecars remain within the existing 4,096-byte bound.
- `authoring-receipt.json`: content hashes and declared offsets, explicitly recording
  unverified alignment, source dimensions and native planning, and `qualified:false`.

Export rehashes the original inputs and refuses replaced or changed files. Linux
publication uses a held parent directory, private staging, file/directory sync and
atomic no-replace rename. Publication failures identify the retained location and
whether output may already exist. Files are private; media is not copied. Reopening
checks the declaration hashes, sidecar coverage and current source/reference bytes.
The receipt is an integrity record, not a signed approval or independent assessment.

Use [saved-view intake](PULSAR_SAVED_VIEW_TRAINING_INTAKE.md) to run the native view
planner and verify decoded source dimensions before staging training inputs. Its
existing assembler accepts the exported manifest and sidecars. A valid declaration
alone does not prove that a video decodes or that the target region fits the resolved
native view. Existing staging budgets and qualification gates remain in force.

## CLI

Both commands are local and do not start an Engine or contact a device:

```sh
pulsar reference-set-create \
  --root /absolute/Live-Reference \
  --video clip.mp4 --reference clip.funscript \
  --view /absolute/clip.view.json \
  --id clip-one --group source-family-one \
  --authorization 'Existing human authorization record' \
  --reference-offset-ms 250 \
  --destination /absolute/new-reference-set

pulsar reference-set-inspect /absolute/new-reference-set
```

CLI creation exports one pair. Use the desktop for multi-entry sets and timing
comparison. The current descriptor-based reader and atomic directory writer are
Linux implementations; other platforms explicitly report unavailable. This does
not close the portable-release gate.

## Verification, 2026-10-24

The new curve contract has eight passing tests. The full client library suite
passes 655 tests (14 ignored), including the new parser, source-binding, publication,
reopen and asynchronous-panel tests. All 132 Core library tests pass. Both strict
Clippy modes, formatting, architecture/source guards and app/worker builds pass.

The [executable interoperability receipt](evidence/roadmap-2026-10-24/reference-set-builder/interop-001/receipt.json)
records the actual CLI, FFprobe, training reader, assembler and native planner using
an authored neutral video and script. It verifies private export modes, exact
offsets, reference interpolation and refusal of both an existing destination and
changed reference bytes. All child commands were joined and no Engine state was
created. The observed video dimensions are 320×180; the old fixture's summary
dimensions were not used as authority. The copied script is deliberately changed
at the end of this refusal test, so this evidence folder is not a ready training set.

The actual egui reference widget is exercised headlessly for painting, correct
frame-time comparison and seeking without changing knots. A rendered native
desktop walkthrough and non-Linux execution remain unverified. No model was
trained and no real-media, human-alignment or physical-device gate was closed.
