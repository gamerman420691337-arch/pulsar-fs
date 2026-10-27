# Offline inert model-pack installation

[Owner-selected offline generation](PULSAR_SELECTED_MODEL_PACKS.md) now supplies a separate explicit execution-approval path. The intake report below remains inert.

The production CLI now imports a directory pack through the authenticated engine:

```text
pulsar model pack-import /absolute/path/to/pack --request-id retained-import-id
pulsar model pack-status exact-manifest-sha256
```

Both operations require the paired session and separate host-local `model-pack-owner.token`. Handy, Stash and project-package capabilities cannot substitute. Import IDs are durable within the paired session: replay returns the original receipt, even if supplier files later change. Retain the exact source path and request ID. Changing the request intent under that ID is refused. Status independently checks the currently installed bytes.

## Directory contract

`manifest.json` is a strict version-1 `ModelPackManifest`. It declares a pack ID, target OS/architecture, ORT API version 24, ordered unique labels, assets, license asset path and supplier description. Each asset declares a portable relative ASCII path, role, exact byte length and lowercase SHA-256. Exactly one `model`, `runtime` and `profile` asset is required, along with a listed `license` asset. Optional roles are `notice`, `native_dependency` and `fixture`.

The `profile` asset contains the existing version-1 `DeclaredInferenceProfile`. Its model/runtime hashes and lengths must bind the listed assets, and its declared class count must equal the number of ordered labels. An identity match alone does not establish ONNX structure, executable runtime validity or output semantics. Captured models now pass the bounded structural/dependency admission below; runtime compatibility and semantic validity remain unproven.

Limits are 128 KiB for the manifest, 1 MiB for the profile, 64 assets, 256 MiB per asset, 512 MiB total, 256 labels and 128 store entries. Paths reject traversal, symlinks, Windows device names, reserved manifest/receipt/staging-marker paths, case aliases and file/directory collisions. Asset files must be regular and have one hard link. Source directory ancestors are walked without following symlinks.

The Linux storage adapter streams and hashes each opened asset into an exclusive private staging directory. It validates the captured profile, syncs files and directory entries, then publishes the entire directory using descriptor-relative `RENAME_NOREPLACE`. The private store is `STATE_DIR/model-packs/MANIFEST_SHA256`; supplier paths are not retained as execution dependencies. Existing packs are reverified and never silently repaired or replaced. Failed ordinary imports remove their staging directory. An abrupt process termination can leave an inert staging entry; automatic garbage collection and disk-quota management remain future work. Those entries count toward store capacity.

Publication includes a filesystem receipt atomically with the bytes. Engine request receipt storage follows publication. If the process fails between them, status by manifest hash reconciles the installed bytes; there is no activation effect to replay. Two actual Linux process-crash points are now exercised: a stopped engine is killed while holding an observed partial fixture-asset copy with no published successor, and a fresh engine is killed after a completed successor import. Restart verifies the prior pack, preserves the inert partial stage, refuses absent-successor status, retries the same request intent into a new complete directory, and preserves completed receipts/bytes through the second crash. Historical replay remains identical after later supplier corruption. This is neither exhaustive crash-point/power-loss/disk-full proof nor cross-platform storage qualification. See the [interruption checkpoint](evidence/roadmap-2026-10-01/pack-import-interruption/validation.json). Other platforms refuse intake until their storage adapter is implemented. Status does not create an absent store.

## ONNX dependency and tensor admission

Import and explicit status inspect the same bounded model descriptor whose length/hash match the manifest, and reject changed descriptor metadata during parsing. Profile bytes are independently length/hash checked when parsed. A 64 KiB buffered streaming parser skips weight payloads without allocating the whole model. Bounds are 256 MiB model bytes, 100,000 parsed fields, 24 recursive levels, 1,024-byte identifiers and 16 tensor dimensions. Truncated lengths/varints, integer overflow, unsupported wire types and ambiguous graph/version/opset/value/type/dimension declarations refuse.

The admitted structural subset requires IR 3–13, declared standard ONNX opsets, one graph and one float32 input/output. Input dimensions are fixed and match the profile's NCHW/NHWC layout. Channel-major detector output is `[1,4+class_count,N]`; end-to-end output is `[1,N,6]`. Candidate count `N` may be symbolic and must still be checked at execution. Temporal position requires named `frame_pair` input `[1,6,64,64]` and `script_position` output `[1,1]`.

Tensor dependencies are walked in main/nested graphs, initializer lists, node tensor/graph attributes and sparse tensor values/indices. Any external-data entry or external data location refuses; no external path is opened. Training bodies, local functions, multi-device declarations, custom node/opset domains and unknown structural fields refuse rather than hiding dependencies. The implementation follows the [official ONNX schema](https://github.com/onnx/onnx/blob/main/onnx/onnx.proto3), including tensor fields 13/14 and nested attribute/sparse containers. This conservative subset is not a full ONNX checker: topology, operator schemas, tensor payload consistency, numerical behavior, provider support and output semantics remain unchecked. No native runtime is loaded to inspect a pack.

Import checks private captured bytes before no-clobber publication. Status independently rechecks installed bytes; old packs containing arbitrary model bytes now refuse status and remain untouched. Mutation replay still returns its historical receipt, so it does not substitute for current status. Existing authority, receipt schema and all inactive/unqualified flags are unchanged. See the [admission checkpoint](evidence/roadmap-2026-10-01/pack-onnx-admission/validation.json).

## Authority and remaining work

The report establishes byte integrity only. `native_execution_approved`, `rights_verified`, `compatible`, `qualified` and `active` are always false. Target matching reports the supplier's declaration; it is not verified provider compatibility. Files are data: installation does not load native code, run hooks, contact a network, train, modify inference selection or supply a fallback.

Still required are authenticated publisher metadata, trust/revocation and rollback policy; executable dependency closure and native trust; full ONNX semantic checking and any future explicitly admitted external-data closure; exact-build notices and redistribution rights; provider known-answer tests; qualification evidence; native-window desktop usability and additional runtime/provider activation; update/removal management; interruption/platform testing; bundled Core/Complete artifacts and relocation/release gates. GB-10 and the full roadmap remain open. Sterile references remain evaluation-only; live-reference training permission does not establish rights for unrelated model packs.

Validation uses authored ONNX graphs and inert runtime bytes and never executes those packs. Native CLI checks now reject arbitrary model data with consistent hashes, truncated protobuf, wrong tensor contracts and external weights before publication. Parser regressions cover hidden nested/sparse/attribute dependencies, every truncated prefix, names/layout/classes, ambiguous/unknown declarations, overflow and recursion/field bombs. Native CLI coverage checks complete capture, original receipt replay, changed-intent refusal, supplier/installed corruption, profile mismatch, symlink/hardlink refusal, restart preservation and no activation. Protocol coverage checks strict schemas, bounds, portable path collisions and global command semantics. Engine coverage checks independent owner/session requirements. Exact final results are recorded separately in the roadmap evidence directory.

## Observed process-crash recovery

`scripts/model-pack-interruption.test.mjs` creates an isolated first-party engine and two authored inert packs. A 256 MiB zero-filled fixture asset is ordered first in the successor manifest to expose a partial-copy interval without production test hooks. Only the child engine owned by the harness receives signals. After `SIGSTOP`, the harness observes `/proc` state `T`, confirms a nonempty partial file smaller than its declaration and an absent successor directory, and hashes the prior pack before `SIGKILL`. The blocked client must fail.

After restart, prior status and bytes must agree, the successor must remain absent, and the orphan stage must match its recorded hashes. Retrying the same request ID and unchanged source produces a verified complete successor while preserving the prior pack and orphan. A second `SIGKILL` after acknowledged completion is followed by fresh status checks for both packs and exact receipt replay, including after intentional corruption of the authored supplier fixture. The test retains actual termination statuses, post-stop filesystem observations and CLI responses. It does not fabricate a crash between rename and database receipt commit.

At that historical interruption checkpoint, orphans were not activated, repaired or removed. They still count toward capacity until an explicitly authorized discard succeeds. Garbage collection, quota management, an active-selection transaction, authenticated publisher rollback policy and all model/native/platform/release qualification remain required. Passing an inert byte-store recovery case does not establish working inference assets or an end-to-end application update.

## Staging ownership observations

New imports create `.pulsar-stage.json` and the captured `manifest.json` before copying assets. The version-1 marker pins the exact manifest hash; the importing process holds an exclusive descriptor lock until import returns or fails. Marker and manifest files/directories are synced before asset copying. The marker remains private engine control metadata in installed packs; manifest asset totals do not include it. Supplier assets cannot occupy its reserved root name. Earlier installed packs passing the current admission checks remain readable without that marker. Legacy packs that declared an asset at the newly reserved marker name now refuse status and remain untouched.

```sh
pulsar model pack-staging-status
```

This authenticated global query requires the same separate model-pack owner capability as import/status. It does not create an absent store, copy assets, remove files, activate a model or infer publisher trust. It scans at most 128 store entries through the private directory descriptor. For each `.pack-stage-` basename, it checks directory ownership/mode, opens a regular single-link current-user/private marker without following links, and tries an immediate exclusive lock. A locked marker reports `locked` with no claimed manifest identity. An unlocked marker reports `unlocked_recognized` only when its strict bounded record agrees with the captured validated manifest's exact hash. Missing, malformed, mismatched, symlinked or otherwise unsafe entries report `unrecognized`; they remain untouched.

These are point-in-time lock/identity observations, not proof that a specific import process is alive or permission to delete an entry. Locks are advisory; a cooperative writer follows the marker contract. Queries serialize behind imports in the current engine, so this is not a nonblocking liveness monitor for that engine. It can distinguish locks held by another process. Legacy stages lack a marker and stay unrecognized. The explicit discard operation below acquires fresh authority, revalidates identity and locks, validates the complete directory tree and excludes published assets before changing anything. No orphan cleanup, quota reclaim, active selection/update/rollback or publisher policy is implemented by this query.

The [ownership checkpoint](evidence/roadmap-2026-10-01/pack-staging-ownership/validation.json) retains the full local gates and native CLI observations of a real crashed partial import, a separately owned process holding its marker lock, legacy and symlinked entries and marker/manifest mismatch. Queries preserve the prior pack, orphan bytes and aliases. This is local Linux evidence, not platform or release qualification.

## Explicit abandoned-stage discard

```sh
pulsar model pack-discard-stage .pack-stage-EXACT_BASENAME EXACT_MANIFEST_SHA256 --request-id retained-discard-id
```

The Linux engine requires a fresh paired session and the separate model-pack owner capability. A staging observation is not deletion authority: discard reopens the exact private stage, reacquires its exclusive marker lock, verifies captured manifest identity and validates the entire remaining tree. Published pack names and inode aliases, locked stages, legacy/unrecognized stages, unexpected contents, symlinks, hardlinks and changed identities refuse. Only declared partial assets and engine control files are eligible. No model selection, native execution or activation follows.

Before changing bytes, the engine reserves the request identity and persists an immutable filesystem intent with the exact directory identity, manifest and planned logical file count/bytes. It renames the stage without replacement into a request-bound quarantine, rechecks identities through held directory descriptors and removes validated files and directories. A synced completion record supports exact replay after restart. Retrying a recognized partial quarantine resumes remaining deletion; a changed intent under the same request ID refuses. `removed` reports the original plan, not measured physical disk space recovered. If durable intent exists but both original and quarantine are absent without completion, `absent_after_intent` records uncertainty rather than claiming successful removal. Malformed intent or completion records fail closed.

Intent/completion records persist in the private `model-pack-discards` directory. New intent admission refuses once that directory already contains 256 entries; temporary/completion writes can exceed that admission threshold. Automatic tombstone retention, quota management, legacy cleanup and general cache eviction remain open. Cooperative advisory locking and private state are assumptions. This is not exhaustive power-loss, disk-full or adversarial same-user filesystem qualification, and does not implement active-version removal, update or rollback.

The [discard checkpoint](evidence/roadmap-2026-10-01/pack-stage-discard/validation.json) records native refusal/preservation, explicit removal of an authored crashed partial stage, exact replay and an actual engine restart after completed discard. Unit tests separately synthesize partial-quarantine recovery and missing-completion ambiguity; they do not represent actual crashes during deletion. Tests touch only isolated authored fixtures. No user cache or real model assets were discarded. See [ADR B6-005](../research/phase-b/ADR-B6-005-explicit-abandoned-stage-discard.md).

[Stash pack batches](PULSAR_STASH_MODEL_PACKS.md) now use exact owner choices and per-scene pack generation with explicit migration and schema-13 reader refusal. These checks do not qualify packs or close distribution gates.

[Persistent desktop pack recovery](PULSAR_DESKTOP_PACK_RECOVERY.md) now journals exact request/intent before dispatch and restores explicit replay without selection authority. Native-window and non-Unix recovery qualification remain open.

The [offline catalog verifier](PULSAR_OFFLINE_CATALOG_VERIFIER.md) authenticates captured metadata/manifests as a primitive. [Linux owner-authorized catalog intake](PULSAR_OFFLINE_CATALOG_INTAKE.md) now retains atomic root/role/clock/manifest and replay facts. [Installed catalog association](PULSAR_INSTALLED_CATALOG_ASSOCIATION.md) now checks exact manifest and asset integrity against unexpired accepted metadata. Selection still acquires no publisher trust; current revocation/update and desktop workflows remain open.
