# Owner-selected model packs for offline generation

An installed pack can now feed the existing confined CPU generation path through an explicit host-owner selection. Import remains inert. Selection records approval for future offline job admission using the engine's already pinned runtime; it cannot approve supplier native dependencies, replacement runtime loading, publisher trust, redistribution rights or learned-model quality.

```text
pulsar model pack-selection-status
pulsar model pack-select EXACT_MANIFEST_SHA256 --expected-selection-revision 0 --approve-unqualified-native-execution --request-id retained-selection-id
pulsar generate /absolute/path/to/video --model-pack EXACT_MANIFEST_SHA256 --output result.funscript
pulsar model pack-deactivate --expected-selection-revision 1 --request-id retained-deactivation-id
```

Read the current selection revision instead of assuming these example revisions. Selection/status/deactivation require a paired session and the separate host-local model-pack owner capability. Generate requires current project Generate and ImportSource grants, a project revision and the exact selected manifest/revision fence. It does not require reading an owner token when invoked through the project RPC. The standalone CLI reads owner selection status to obtain that exact fence and profile before submitting generation.

## Admission and retained evidence

Selection rechecks the installed asset/receipt closure, declared target OS/architecture, canonical snapshot model/runtime identities and the implemented CPU preprocessing/resource envelope. The profile must bind the engine-pinned runtime exactly and use two CPU threads. Packs declaring supplier native dependencies refuse selection. The supplier runtime asset remains integrity-checked data; it is not dynamically loaded by this operation. A pack for another runtime/provider/target can remain inertly installed while selection refuses it.

Generation independently repeats current integrity/admission checks. The opened model descriptor feeds immutable snapshot capture; its resulting identity must equal the declared model identity. Conflicting caller profiles/input contracts refuse. The manifest, original profile bytes and recorded selection approval enter private content-addressed snapshots and the worker dependency list before the initial checkpoint and SQL job publication. Approval evidence includes the selecting session and request identity. Retry retains these captured dependencies. Portable export/clone retains their bytes as historical evidence without creating a new job or activating a pack.

The final admission transaction rechecks authentication, project grants/revision and the current selection fence. Changing selection during preparation cannot publish a stale job. Selection changes use optimistic revisions and durable request replay. Replaying an older successful selection returns its historical receipt without reactivating it. Deactivation affects future admission; already accepted jobs retain their immutable inputs. This is not a cancellation operation.

`pack-status` remains the inert integrity report with `active=false` and `native_execution_approved=false`. `pack-selection-status` is the separate recorded owner approval/fence/profile. It is not a fresh filesystem verification or a runtime-quality result; generation performs the fresh checks. Selection reports retain `rights_verified=false`, `compatible=false` and `qualified=false`.

## Reader and local test boundaries

The first stored selection or deactivation advances the SQLite reader boundary to schema 12 in the transaction containing the approval and replay record. Startup validates that table before writable SQLite startup. Source-placement initialization, imported interval recovery and format-9 export/import preserve the newer boundary. The portable format remains 9: the worker contract is unchanged, and additional opaque dependency evidence grants no execution authority.

The [local receipt](evidence/roadmap-2026-10-01/model-pack-selection/validation.json) records the final checks. Rust cases exercise durable revision fences, corrupted storage refusal, separate owner/session requirements, replay and intent conflict. The native fixture uses an authored ONNX graph, identified CPU runtime and generated video. It executes a real confined worker, verifies exact pack/profile/approval snapshots, refuses changed model bytes and caller input, exports/clones the generated origin, deactivates without creating new jobs/attempts, checks historical selection replay, restarts and checks preceding-reader refusal with database main/WAL hashes unchanged. SQLite's volatile shared-memory lock/index bytes are outside that preservation claim.

[Compiled desktop controls](PULSAR_DESKTOP_MODEL_PACKS.md) now have headless draw/input and command-state checks. [Detector preview](PULSAR_MODEL_PACK_PREVIEW.md) now binds exact selection/model/profile and source or placed requests. [Stash batch pack admission](PULSAR_STASH_MODEL_PACKS.md) now records exact choice/profile and explicit migration. [Persistent desktop pack recovery](PULSAR_DESKTOP_PACK_RECOVERY.md) now retains session-bound intent before dispatch. Native-window usability, trusted publisher/update policy, automatic update/removal management, bundled assets, additional runtime/provider/platform adapters, learned-model quality, clean-machine distribution, Handy and human/release acceptance remain open. These local controls and authored fixtures do not complete GB-10 or the full roadmap.

## Catalog-bound future admission

[Explicit catalog-bound approval](PULSAR_CATALOG_BOUND_APPROVAL.md) now offers an additional Linux owner action retaining exact bootstrap namespace/catalog revision. Future admission requires fresh membership/expiry/clock checks and explicit reapproval after catalog changes. This conditional version-2 report raises the reader barrier to 14 and preserves it after ordinary deactivation/selection. Ordinary unqualified selection retains its existing version-1 contract; neither mode establishes publisher rights, provider/model qualification or online revocation freshness.
