# Portable Stash source associations and explicit rebinding

Status: local source-association implementation; broader portable publication/recovery and qualification remain open.

A portable project now retains a captured media source's Stash endpoint, scene/file IDs and server path, plus the exact original capture receipt as a hash/length-bound dependency object. Original host paths in that receipt are inert archival text. There are no credentials, sessions, grants, owner proof or destination live Stash association in this closure. Capture is performed under the existing project snapshot/export authority and does not contact Stash or follow receipt paths.

Sources may declare at most 16 associations, sorted by distinct receipt digest. Each receipt is at most 64 KiB; engine capture bounds its SQLite cells before allocating strings. Logical validation requires exact declared dependency roles and media sources. Typed import checks each hash-verified receipt against its source identity, source version, endpoint, scene/file/server path and sibling-script declaration. This verifies correspondence only; an imported declaration is not independent proof of remote truth or publication permission.

New associations require portable format 8. Simple projects without associations or format-8 ancestry retain their earlier minimum format. Clone/re-export preserves original receipts and their object bytes through origin namespace maps. Origin validation refuses dropping an original association; new explicitly captured associations may coexist with archived declarations. The exact pinned preceding format-7 reader refuses format 8 before publishing a project. Database schema stays 9. Import creates no live `stash_sources` row or job, schedules nothing and does not resolve archived paths/endpoints.

To bind a currently mapped Stash file to an existing imported media source, select the current server/mappings and name the expected project source explicitly:

```sh
pulsar stash capture-source PROJECT SCENE FILE --url CURRENT_GRAPHQL_URL \
  --mappings current-mappings.json --expected-source-version SOURCE_VERSION \
  --revision CURRENT_REVISION
```

The engine requires current local owner capability, project ImportSource and Read grants, current revision and an existing immutable media source. An absent or inconsistent source refuses before contacting Stash. The broker re-queries the selected scene/file and captures the current mapped descriptor without following symlinks. Different source bytes refuse before catalogue/association publication. Length, authority, revision and source identity are rechecked in the transaction. Successful rebinding retains the same content source version and creates only the current active association. Failed preparation may leave an inert charged snapshot. Rebinding an existing source is allowed at the 128-source catalogue limit; creating another source is still refused. No master motion/history/protection or generation job changes.

The compiled desktop Stash panel exposes “Require capture to match selected project source” and uses the same command. Native GUI rendering/usability remains unverified. Ordinary capture without the option keeps its earlier explicit new-source capture behavior and wire representation.

After rebinding, a fresh native generation can be merged and published through the existing Stash source/job/candidate correspondence and no-clobber rules. Imported historical jobs/candidates remain archival; this increment does not activate them, import historical publication/scan/tag authority, recover lost remote job IDs or migrate original source-clock publication proofs. Publication of an edited historical imported candidate under its original archived generation requires separate explicit lineage/clock-proof migration work. Fresh generation is not a substitute claim for that requirement.

The [local receipt](evidence/roadmap-2026-10-01/portable-stash-associations/validation.json) binds exact source/binaries and local gates. Its authored native fixture captures, exports, clones twice, re-exports and restarts without another Stash query or live import association/job. Forged/omitted archival metadata refuses before project publication. Missing binding refuses before query; changed bytes leave catalogue/master/association unchanged. An owned fixture catalogue-length fault also refuses before active association publication. Matching explicit rebinding creates one fresh generation job and successful local sibling publication. The preceding reader independently refuses format 8. All server requests are queries to an owned mock; supplier model/media stay unchanged except the fixture's explicit temporary mutation/restoration. No installed Stash, private reference, training, model-quality, Handy, GUI/platform/human or release qualification follows.
