# Archiving analytic device-motion exports

Portable project capture now preserves analytic device-motion export history. Previously,
`export-device-motion` recorded the whole proposal as `device-motion`, but package capture
tried to decode that label as one of the six motion axes. A valid export therefore caused
subsequent package export to fail with `unknown variant device-motion`.

An archival export scope now distinguishes a selected axis from a whole device-motion
proposal. The latter may contain one or several axes; capture does not guess which from
a filename or mutable destination. Its revision, output hash, publication state and private
archival path remain historical metadata. Capture and import never reopen that output path,
republish the file, restore device commands or grant qualification.

## Compatibility

Packages containing a device-motion export receipt use container/manifest format v14.
Retained v14 import origins also keep that minimum version on re-export. Older readers
must refuse the version before consuming the new meaning. Unknown future versions and
unknown scope labels remain rejected.

Ordinary axis receipts retain their exact scalar JSON spelling and field layout, and
packages without v14 content retain their existing required format. The historical wire
field is still named `axis`; its Rust type is now `PackageExportScope`. Core's six-axis
model is unchanged. Neutral-motion evidence verification accepts only the matching
single-axis scope, even if a composite receipt has the same file hash.

No database rewrite or destination-file inspection is needed. Existing complete, pending,
and failed-or-unknown device-motion export rows are captured as recorded. A completed
file publication is not evidence of physical execution, stopping, suitability or human
acceptance. Imported receipts remain inert; published output files are not package payloads.

## Usage

The existing `export-device-motion`, `package-export`, `package-import` and package download
commands remain the interface. Use the rebuilt app/worker and current clients for v14
packages. No device credentials or network access are needed for this archival path.

See [decision](../research/phase-b/ADR-B5-026-device-export-archival-scope.md) and
[checkpoint](evidence/roadmap-2026-10-16/device-export-archive/checkpoint.json).
Handy hosting/setup, qualified playback and the remaining model/media/platform/device and
human release gates remain separate unfinished work.


## Scoped verification

The original native sequence passed without a device proposal, then failed twice after
one successful proposal export. The focused two-axis regression also failed with the
same unknown-variant error before the fix and now passes after removing the external
proposal file. This rules out using the mutable output as a repair or provenance source.

The complete Protocol suite passes 309 tests. Engine package tests pass 95 with one
existing qualification case skipped, the focused composite regression and Handy CSV
capture regression both pass, and Client package tests pass 31. Strict default/all-feature
workspace lint, App/Worker builds, format and architecture checks pass. Ordinary codec
golden hashes remain unchanged. V14 round-trip, future-version refusal and rejection of
composite receipts as neutral-axis evidence are covered.

The isolated native campaign verifies actual current CLI/Engine package creation,
current import and re-export, restart preservation, and unchanged complete/unknown
publication states. A retained preceding client accepts the ordinary package and refuses
v14 without creating a project. The original published proposal is deliberately replaced;
its archived hash is retained and the replacement is never read as evidence or overwritten.
Three fixture projects exist (source plus two clones), with only the source's one job and
attempt. No executable job is restored for the v14 clone. Both Engine processes are joined
and worker custody is empty. The preceding binary's identity is retained by reference;
no additional copy was made.

This proves the scoped archival behavior on the tested Linux environment. It does not
qualify all older Engine database readers, all platforms, complete ordinary-storage
suites, device dynamics or physical playback. No real media, credentials, networked
service or device was used. The roadmap remains partial.
