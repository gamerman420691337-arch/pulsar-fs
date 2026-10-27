# Local library companion-axis inventory

`pulsar library-axes FOLDER [--recursive]` observes companion paths without an
Engine connection, generation, export, or file adoption. The desktop library
panel's **Inspect companion axes** runs the same code on its background client
thread, including while a previous mutation needs recovery.

For each discovered media entry, all six neutral axes are reported independently.
Recognized locations are `<media filename>.pulsar-audio-bundle/<axis>.funscript`,
the adjacent `<media stem>.funscript` for stroke, and
`<media filename>.pulsar-visual-bundle/stroke.funscript` for stroke. Other vendor
or secondary-companion naming conventions are not inferred.

- **present**: at least one recognized regular file was observed, with no blocker.
  Empty, malformed, or stale regular files still count as present and remain
  protected; their contents are not parsed or qualified.
- **missing**: all recognized locations were absent at observation time.
- **blocked**: a recognized leaf is a symlink/non-file, a bundle is a
  symlink/non-directory, or metadata cannot be read. A blocker remains visible
  even if another recognized companion is present.

The report retains every recognized path and its individual observation, plus
present/missing/blocked channel counts. Discovery uses the existing bounded media
scanner (100,000 directory entries and depth 64); this inventory refuses more
than 100 media entries. It accepts an ordinary directory root, skips discovered
symlinks, and reports directory traversal errors rather than silently omitting
them. No recipe, model, annotation, or credential is needed.

These are mutable path observations, not a coherent filesystem snapshot, content
hashes, ownership proofs, or authorization to publish. Concurrent replacement
can invalidate them. A future partial-axis publisher must capture and recheck
protected paths and enforce no-overwrite behavior itself. This inventory does
not fill missing axes, merge an existing bundle, change retained batch intake,
or qualify TEST-15's generation behavior. The later [selected-audio workflow](PULSAR_LIBRARY_SELECTED_AUDIO.md) supplies separate evidence for the adjacent-stroke case; the later [partial-bundle workflow](PULSAR_LIBRARY_PARTIAL_BUNDLES.md) supplies separate evidence for existing-directory fills.

The native campaign uses authored placeholder media and companion bytes; no
media decode or useful-script claim follows from it. Exact source identities,
checks, native results and cleanup are retained in
[the checkpoint](evidence/roadmap-2026-10-12/library-axis-inventory/checkpoint.json).
Roadmap status remains PARTIAL; annotations, useful models, real-media quality,
native rendered GUI/platform, physical Handy and human acceptance remain open.

Scoped validation passes: 398 client library tests plus two fixture tests, ten
ignored; both strict lint configurations, Windows source check (existing
warnings), native build, formatting, architecture and 36 guards. Five native CLI
commands verify mutable observations, protected bytes, no Engine state, recursive
selection and refusal. Engine/Protocol source identities are unchanged; their
suites were not rerun. No rendered-window acceptance is claimed.
