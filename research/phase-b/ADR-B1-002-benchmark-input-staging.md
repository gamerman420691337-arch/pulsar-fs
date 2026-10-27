# ADR-B1-002: private benchmark input staging

Status: implemented intake foundation; production campaign execution remains open.

## Decision

The campaign intake consumes the existing B1.1 corpus plus a separate source map.
The source map contains schema_version 1 and entries with entry_id and a path
relative to the map. It must cover the selected development or heldout split
exactly. Existing corpus validation checks permissions, leakage groups, source
digest collisions, reference provenance and heldout freeze declarations before
staging. These records remain human assertions requiring their own audit.

Each source is streamed from an opened regular-file handle into a new private
directory while hashing the actual copied bytes. Explicit per-source and aggregate
source byte limits, bounded JSON/reference inputs and a checked deadline constrain
admission. Symlinks escaping the declared input root are rejected. Source paths
are never used as output names. Reference annotations must match their declared
digest and subject. A failed invocation removes only its newly allocated directory.

The result preserves source and reference hashes, original declarations, selected
split, corpus/map digests and budgets. Files are made read-only. Consumers must
verify hashes again: owner-writable storage is not an immutable trust boundary.
The final intake.json identifies a completed staging attempt, but no filesystem
crash-durability qualification is claimed. Incomplete directories after a crash
must not be admitted without a valid receipt and verified files.

## Usage

```sh
node scripts/benchmark-intake.mjs \
  --corpus /permitted/corpus.json --sources /permitted/sources.json \
  --output-root /private/benchmark-runs \
  --max-source-bytes 1073741824 --max-total-bytes 10737418240
node --test scripts/benchmark-intake.test.mjs
```

## Remaining item 2 work

The existing corpus candidate declaration is retained as metadata but is not
read or executed during intake. No candidate, runtime execution, human session or
quality result is manufactured. Production worker invocation, exact dependency
profiles, independent conversion of raw motion to evaluation annotations, bounded
runtime supervision and human-receipt intake remain required. The execution field
is null and release_qualified is false. This foundation does not close item 2.

Tests use authored bytes and annotations, including failure cleanup, mismatched
identities, rights, budgets, leakage and unchanged earlier snapshots. They are
intake-contract evidence, not detector or media-generation evidence.
