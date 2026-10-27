# Annotated Stash batches with owner-selected model packs

Status: implemented with authored native evidence. Roadmap PARTIAL.

Annotated Stash batches accept either an explicit file model/profile or an explicitly owner-selected annotated CPU or owner-authorized CUDA pack. Every eligible selected source still needs an exact scene/file/hash/eye/projection/target catalog entry. The batch template has no single-source view; each pending generation receives the matching captured source binding. Optional [per-source placement/interval plans](PULSAR_STASH_SOURCE_PLANS.md) now compose with the catalog and retain explicit source-time publication. No target is inferred from a pack.

## Invocation

```sh
pulsar model pack-import SUPPLIER_DIRECTORY
pulsar model pack-select MANIFEST_SHA256 --expected-selection-revision REVISION \
  --approve-unqualified-native-execution
pulsar stash generate-missing --url http://127.0.0.1:9999/graphql \
  --mappings MAPPINGS.json --model-pack MANIFEST_SHA256 \
  --annotated-views VIEWS.json --width 64 --height 64 --limit 2 --detach
```

`VIEWS.json` uses the existing [per-source annotated catalog](PULSAR_ANNOTATED_STASH_BATCHES.md). Pack and file model/profile arguments remain mutually exclusive. The CLI loads the selected profile and validates every resolved view before submission. The Engine independently checks catalog settings and selected eligible coverage before capturing the annotated pack model. Existing unsupported file-model providers still refuse before discovery. Legacy pack batches retain their earlier preparation order.

## Authority and recovery

Pack preparation verifies current owner choice, installed bytes, Engine-pinned runtime, implemented tensor adapter and exact caller/profile/input correspondence. Batch preparation validates every catalog entry against the selected profile before model snapshotting; ordinary generation still requires a complete single-source view. Per-source dispatch rechecks current selection and retains exact pack manifest, profile and owner-approval documents through the existing annotated job and receipt barriers.

The existing version-5 batch ledger and conditional pre-recovery reader requirement apply. There is no new wire schema. This newly permitted combination is rejected by preceding readers that require file-only catalogs. Migration remains owner-authorized and ledger-CAS-bound, retains previous catalog/pack intent, and stays paused across restart until explicit resume. A new selection revision requires explicit replacement intent; restarting cannot silently adopt it.

## Native CPU evidence

The successor [CUDA pack batch checkpoint](PULSAR_ANNOTATED_CUDA_PACK_BATCHES.md) verifies the same owner and recovery obligations with actual CUDA workers, including placed/interval selected-pack batches.



The shipped CLI/Engine campaign generates and publishes two distinct source hashes with different left/right views, restarts after durable generation intent, and retains one generation request per source. Every job retains the exact pack manifest, profile and owner-approval dependency. A third wrong-hash source stops before any generation request or job. Stale migration and withdrawn owner choice refuse; explicit replacement with the current selection revision preserves prior intent, remains paused through restart, and completes only after explicit resume.

All three completed jobs preserve 28 decoded samples each. All 42 checked unrounded positions match the independent Python oracle exactly, and all 42 rounded script positions agree. A fourth source is held during capture while pack withdrawal is queued; its subsequent generation refuses the removed approval and creates no job or output. The database contains exactly three jobs. The actual preceding executable refuses the new ledger combination while observed database/WAL bytes remain unchanged. This does not assert that every filesystem path is untouched.

The final workspace gate passes 1,099 tests with 62 ignored; the Engine subset passes 591 with 54 ignored; CUDA-feature model-pack checks pass eight. Four preceding native campaigns covering annotated file-model batches, direct annotated packs and legacy packs also pass. The default build, 284 architecture checks and 36 source guards pass. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-stash-pack-batches/checkpoint.json). The packet retains the initial executable-change refusal and the admission-order regression exposed by tests, followed by the corrected final gates.

## Limits

Selection approves unqualified native execution only. Rights, compatibility and qualification flags remain false. The native campaign uses authored stereo media, an independent Python oracle and an owned query-only loopback Stash fixture. It does not contact the user's Stash, use live or sterile training inputs, fit a model or actuate hardware.

Annotated CUDA preview/desktop workflows and human usability, mixed-clock publication without piecewise proof, broader fault/retention qualification, actual Stash and desktop compatibility, useful learned quality with held-out sterile evaluation, offline/platform release distribution, physical Handy playback and human/release acceptance remain open. Placed/interval batch implementation and authored native evidence are recorded separately in the source-plan checkpoint; they do not qualify these remaining gates.
