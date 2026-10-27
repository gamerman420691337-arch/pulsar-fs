# Portable-project runtime checks on the current Linux build

All seven explicitly deferred Rust cases were executed against the recorded portable-rebase source and passed. The default workspace suite still marks them ignored because they require pinned historical binaries, explicit worker execution or over 1 GiB of private generated data. Their separate execution now has a [source-bound receipt](evidence/roadmap-2026-10-01/portable-runtime-qualification/validation.json), six raw logs, host observations and exact executable byte identities. Production and verification source are unchanged from the portable-rebase checkpoint. The subsequent [attribution audit](PULSAR_REBASE_ATTRIBUTION_AUDIT.md) corrects the preceding checkpoint's mislabeled negative-test binary; the current positive binary and the seven runtime results remain valid.

## Large transfers

Both authored fixtures use 1,074,003,968 bytes of generated source. Export produces a 1,074,012,662-byte package; import consumes a 1,074,012,672-byte package and verifies a re-export. Each crosses the old 4,096-chunk boundary with at least 4,098 chunks of at most 256 KiB. The shared helpers retain exact byte counts, verify complete content, and publish durably. Import verifies exactly one new project, preserved motion, immutable retained original container, source identity and namespace/container correspondence in the re-export.

| Measured subject | Export/download fixture | Import/re-export fixture |
| --- | ---: | ---: |
| Capture/import plus initial export | 2.959 s | 2.927 s |
| Prepared input hash | — | 0.541 s |
| Download / import | 1.172 s | 9.381 s |
| Re-export | — | 3.929 s |
| Sampled engine RSS delta | 3.48 MiB | 2.31 MiB |
| Sampled client RSS delta | 1.25 MiB | 0.41 MiB |
| RSS samples | 251 | 494 |
| Snapshot control latency | 0.308 ms during streaming | 1.295 ms maximum observed |

The import run observes 815 upload, 224 validation and 623 other active control probes. Its maximum observed control response is 1,300 bytes. Both active upload and validation phases are required; an early Ready result cannot skip responsiveness checks.

RSS values are sampled process growth relative to the fixture baseline, not total memory, formal heap bounds or reference-hardware qualification. Latencies belong to this host and these generated-data workloads. They do not measure concurrent perception, GPU overload, real-media generation or device scheduling. Successful fixtures remove only their own private generated temporary data. No user media or cache is cleaned.

## Real worker and historical readers

Two cases execute the actual confined worker with the strict synthesized text recipe `hold`, stroke axis, two seconds, offset 0.42. They verify exact generated source/raw program/receipt identity, original historical job and attempt facts, and unchanged uncommitted source state. The clone case retains the original bytes while creating zero imported jobs and attempts; explicit candidate commit and undo/redo remain local actions. Receipts retain `qualification=unqualified`. This is actual deterministic worker provenance, not learned-model quality or physical evidence.

Three historical-reader cases execute pinned schema-2/schema-3 binaries. They verify package refusal before creation of a new project and current database refusal followed by reopening with preserved placement membership, revision and motion. The database helper creates placement data only: the schema-3 test's timing-proof name does not mean it constructs timing-proof payloads. Actual retained-clock proof reader cases are recorded separately in the publication checkpoints.

## Remaining acceptance

The subsequent [owner-selected model-pack increment](PULSAR_SELECTED_MODEL_PACKS.md) changes production source. These seven results remain bound to this receipt's archived source; they were not rerun for that increment.

These seven local cases no longer lack current execution evidence. Default annotations remain intentional. The earlier full checkpoint still supplies 825 ordinary Rust passes, 91 native-configured JavaScript passes and 201 structural checks; it is not represented as a fresh full-suite run here because source did not change.

Exhaustive crash/power-loss/disk-full behavior, formal resource bounds, other platforms and release packaging remain open. So do mixed-clock correspondence, model/media quality, rendered GUI usability, installed Stash behavior, physical Handy and human/release acceptance. The full roadmap remains incomplete.
