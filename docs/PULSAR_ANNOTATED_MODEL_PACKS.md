# Owner-selected annotated CPU model packs

An offline pack can now carry the existing annotated temporal CPU profile and its exact ONNX recipe. Import/status use the shared bounded annotated ONNX parser, validating the named frame-pair ABI, recipe contract and intake digest in the already hashed model bytes. This is dependency/tensor/metadata admission, not a complete ONNX graph checker, rights verification or model-quality qualification. Legacy packs keep their previous parser and provider paths.

Explicit owner selection still verifies installed bytes, the current host target, canonical model/profile identities, the Engine-pinned runtime and implemented CPU tensor adapter. The supplied runtime asset is verified against that runtime identity; no supplier native dependency is automatically executed. Selection remains separate from import and requires the existing explicit unqualified native-execution approval. Rights, compatibility and qualification flags remain false.

## Generation

```sh
pulsar model pack-import SUPPLIER_DIRECTORY
pulsar model pack-select MANIFEST_SHA256 --expected-selection-revision REVISION \
  --approve-unqualified-native-execution
pulsar generate SOURCE.mkv --model-pack MANIFEST_SHA256 \
  --annotated-view VIEW.json --output NEW_OUTPUT.funscript
```

The manifest remains the existing exact-byte schema-1 pack contract. `VIEW.json` is the same source-hash/eye/projection/target-region declaration used by [direct annotated generation](PULSAR_ANNOTATED_PUBLIC_GENERATION.md). CLI grammar now permits that view with either an explicit file profile or selected pack, retaining conflicts for competing views and raw/target inputs. The selected profile supplies its explicit 64x64 input contract; no target is inferred from a pack.

The Engine checks imported-media kind, current revision and exact view/source binding before pack model capture. Preparation verifies the current owner choice and caller/profile correspondence, validates complete settings before snapshotting model bytes, and retains exact pack manifest, profile and owner-selection documents as dependencies. The existing confined worker, schema-23 immutable membership, schema-3 receipt/model verifier and candidate publication checks remain authoritative. An annotated pack without a view refuses. Legacy detector Preview continues to refuse temporal profiles. The distinct [source-bound annotated Preview](PULSAR_ANNOTATED_PREVIEW.md) now supports explicit CPU pack and file-model requests.

## Native and portable evidence

The authored campaign imports and explicitly selects an annotated pack, generates through the shipped CLI/local IPC and real CPU worker, commits and exports 28 actions. All 14 intended unrounded Python estimates match exactly; all 14 rounded positions agree. The job retains the exact source declaration, model identity and original pack/profile/selection document bytes. Its schema-3 receipt and unqualified status survive an Engine restart.

Actual format-11 CLI download/import passes. The clone re-exports while the original supplier directory, installed pack directory and original model snapshot path are unavailable; a second clone imports and opens after restart. Only the original generation job exists: imported pack-selection provenance grants no active job or new owner-selection authority.

Rehashed changed-recipe and graph-input-name packs refuse. Missing view and wrong source hash refuse, with the latter leaving the pack model snapshot absent. Installed model drift and deactivated approval refuse future generation without extra jobs or sibling scripts. The original candidate survives these changes. An actual preceding executable refuses annotated pack import through its existing explicit guard. No new database schema or pack format is needed for this capability; existing annotated job/portable barriers protect execution history.

The initial native harness mutated only a node use of the input name, which is outside the bounded ABI parser's complete-graph-checking claim. The negative fixture was corrected to change the graph declaration too; its expected diagnostic was then corrected to the actual ONNX name/shape refusal. No production validation was weakened.

Final checks: 1,098 workspace tests pass (62 ignored), the annotated native pack/portable campaign and two legacy native pack campaigns pass, the default binary builds, and 284 architecture checks plus 36 source guards pass. See [ADR B3-077](../research/phase-b/ADR-B3-077-annotated-model-packs.md) and the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-model-packs/checkpoint.json).

This is authored native execution and portable correspondence, not natural-media quality. No fitting, live/sterile input or physical playback occurred. [Annotated Stash pack batches](PULSAR_ANNOTATED_STASH_PACK_BATCHES.md) are now implemented with separate source-bound evidence. Annotated CUDA, actual desktop window rendering and human usability, placed/interval batch orchestration, mixed-clock publication without piecewise proof, broader fault/retention work, actual desktop usability, real annotations/alignment and useful held-out quality, platform/offline release distribution, physical Handy and human/release acceptance remain open. Roadmap status: PARTIAL.
