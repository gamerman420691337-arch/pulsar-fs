# Explicit retained-runtime selection

Linux x86-64 now supports verifying and launching a retained runtime by its original manifest SHA-256 without reopening the supplier directory or archive. This supports an explicit return to older runtime assets after a failed update or removal of the original download.

```
pulsar runtime-cache-inspect /absolute/cache-root MANIFEST_SHA256
pulsar runtime-cache-launch /absolute/cache-root MANIFEST_SHA256 -- --version
pulsar runtime-cache-launch /absolute/cache-root MANIFEST_SHA256 -- gui
```

Use the exact 64-character lowercase digest retained in the original manifest-addressed installation directory or installation receipt. Selection never chooses a latest version. Inspection does not execute the selected retained application or initialize Engine state. An AppImage first prepares its own bundled runtime through ordinary first use before forwarding these commands; retained selection itself creates no cache entries. Graphical launch uses the setup window; headless forwarded commands preserve their existing path.

The host opens existing trusted cache ancestors without following symlinks or creating missing components. It requires the existing private single-link lock, refuses active preparation, validates the finite manifest schema and its selected identity, then rechecks every payload, mode, ownership, link count, receipt and exact inventory. Unknown or damaged contents remain available for inspection. Missing entries and locks are not repaired. Selection changes no active pointer and removes no retained version.

After verification, the composition root executes the selected cached application with transient AppImage variables removed. Normal authenticated protocol negotiation remains authoritative for an existing Engine. The launcher never stops or replaces that Engine. An older client may communicate with a compatible running Engine; returning to runtime assets is therefore not a promise that an older Engine is running. This operation does not roll back the project database, migration state, model activation, settings or authored history. Incompatible schemas, signed update trust, retention/garbage collection, delta distribution, release compatibility policy and full interruption/storage/platform qualification remain open.

The [checkpoint](evidence/roadmap-2026-10-11/runtime-cache-return/checkpoint.json) records current source, binary and process evidence. The actual process campaign uses two retained runtime subjects, hides both supplier directories, refuses a damaged update, returns to the older verified assets, preserves unknown files and checks the existing Engine PID/executable. This does not establish useful learned motion, physical Handy behavior or human release acceptance.

Verified results: 784 Engine tests, 229 client tests and four composition-root tests pass; both strict Clippy configurations, all-features binary build, formatting, 382 architecture checks and 36 source guards pass. Four final packaged process tests pass with zero skips. The earlier binary refuses the new command; an initial packaged harness run passed three tests and failed one because it hid its own selector. Both failures remain recorded. Final hidden-supplier selection uses a private selector outside both supplier mounts. All fifteen assembled payload identities were independently rechecked.

Retained AppImage: `/home/brianklam/Projects/Funscripts/assets/pulsar-runtime-cache-return-2026-10-11/Pulsar-runtime-candidate.AppImage`, 710420984 bytes, SHA-256 `c11ebdf70a6fc72490f4d501f6b74441af57af32042f9a1b1603bdeba0b90e6b`; manifest `707625b8208988667742789f77db537f7edc904a02bc4e68e0d0097e250a3e60`. Own scratch was removed after terminal jobs and accessible process-reference checks; inaccessible/transient scan outcomes remain recorded. Prior assets and shared caches are preserved.
