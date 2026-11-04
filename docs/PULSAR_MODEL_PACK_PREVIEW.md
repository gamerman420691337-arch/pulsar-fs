# Owner-selected detector preview

The Generator and CLI can preview a detector from the exact current owner-selected offline pack. The command carries the manifest digest and selection revision, source/frame/view generations and an explicit source-time or placed-time request. It accepts no loose model path. Source preview uses elapsed time from the first frame; placed preview uses the project's current placement and project time.

```sh
pulsar project preview-pack PROJECT_ID 500000000 --model-pack EXACT_MANIFEST_SHA256 --source-version SOURCE_VERSION_ID
pulsar project preview-pack PROJECT_ID 750000000 --model-pack EXACT_MANIFEST_SHA256 --placement-id PLACEMENT_ID
```

Selection remains explicit approval for unqualified offline native execution through the engine-pinned CPU runtime. The interface now describes both analysis and generation. Preview additionally requires current project Read and ImportSource grants. Inert intake alone grants no execution permission.

The engine repeats pack asset/profile/runtime admission, captures the opened model and manifest/profile/approval documents, binds canonical profile/model/runtime identities in the finite preview worker dependencies, and applies the actual preview resource budget. It checks selection before registering work and again before returning its result. A selection change invalidates completion without cancelling an already captured worker. Current grant and placement checks, actual decoded source time/ordinal, view mapping and output byte verification remain the shared preview contract.

The response identifies the exact choice, model and canonical profile digest and retains `qualified=false`. The CLI checks these against its queried selection and verifies the requested time/view/context. The desktop additionally requires its current pack mode/profile/choice and existing preview request association before displaying pixels or observations. Historical selection receipts cannot populate its current choice. No durable generation job or attempt is created by preview; finite worker output still occupies the existing private attempt directory.

Only the implemented single-frame detector decoder is admitted. Temporal script-position packs require a frame pair and keep pixels-only preview. [Stash batch pack binding](PULSAR_STASH_MODEL_PACKS.md) is now implemented through its own durable admission/migration contract; [persistent desktop pack recovery](PULSAR_DESKTOP_PACK_RECOVERY.md) is now implemented with explicit session-bound replay; native-window qualification remains open. No new database or portable format is introduced: preview itself leaves schema 12 and format 9 unchanged; later pack batches conditionally advance the database to 13.

The [local receipt](evidence/roadmap-2026-10-01/model-pack-preview/validation.json) retains exact source/executable identities, workspace checks and authored native observations. The fixture runs real CPU detector preview at source frame 4 and placed frame 6 in both directions, then refuses changed model bytes and deactivation without extra durable jobs/attempts. Unit cases exercise project permission, completion fencing and stale model/profile/frame/qualification rejection. The initial new fixture incorrectly expected canonical profile metadata to have a CAS file; its corrected check compares the dependency identity and actual declared profile. Raw failure and final pass are retained.

These are local software checks with generated media and an authored ONNX graph. Native-window usability, installed Stash, learned-model/real-media quality, Handy, other platforms and human/release acceptance remain open. Sterile evaluation and training were not rerun.
