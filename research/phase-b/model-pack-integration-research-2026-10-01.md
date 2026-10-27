# Offline model/runtime packs: contracts and release evidence

Research date: 2026-10-01. Scope: packaging/import design, current detector contracts, and owning-source license/compatibility evidence. No weights downloaded, models trained, user services contacted, binaries installed, release published, or unrelated files modified. This document records source terms and implementation implications; it does not grant redistribution rights or qualify a model.

## Current Pulsar adapter contract

Inspected the active working tree read-only:

- `crates/pulsar-engine/src/worker/detector.rs` loads an exact runtime path through `ort::init_from`, creates a strict CPU EP session, and sets declared intra-op threads / one inter-op thread. Input is float32 `[1,3,H,W]`, RGB nearest-neighbor **stretch**, byte/255, no bias or padding. Width/height must be positive and ≤1024; classes positive and ≤256. One float32 output is required.
- `crates/pulsar-core/src/perception.rs` channel-major decoder requires `[1,4+classes,N]`: pixel-space center x/y/width/height followed by per-class scores, without a separate objectness channel. It validates finite elements/extents, caps rows at 100000, clips and applies deterministic class-aware NMS. Core also knows an end-to-end schema, but the production worker does **not** admit that decoder.
- `crates/pulsar-engine/src/inference_profiles.rs` binds model/runtime byte identities and exact preprocessing. It rejects non-CPU, letterbox/BGR/other normalization, and thread mismatches. `DeclaredInferenceProfile` is schema version 1; structural admission and digest matching are not model/provider qualification.
- `crates/pulsar-engine/Cargo.toml` uses `ort 2.0.0-rc.13`, dynamic loading, and `api-24`. CUDA/TensorRT compile features exist, but detector admission/execution remains CPU. A manifest must not advertise accelerated execution merely because a feature compiled.

Legacy `pulsar/src/neural/yolo.rs` assumes 10 anatomy classes and documents `[1,14,8400]`, with the same stretch/RGB normalization. Legacy model manager's default is `FunGen-12n-pov-1.1.0.onnx`; it calls files larger than 1 MB valid, and its registry uses `sha256:"pending"` for all named families. SAM/YOLO26/RF-DETR/TAPNext/CoWTracker names and performance descriptions are not installed, licensed, compatible or qualified pack entries. Class-index meaning must come from the exact model/export, not the legacy fixed names.

## Existing default model identity and rights

The [official release asset API](https://api.github.com/repos/ack00gar/FunGen-AI-Powered-Funscript-Generator/releases/tags/models-v1.1.0) reports asset ID `430900051`, `FunGen-12n-pov-1.1.0.onnx`, 10576223 bytes, digest `5dc012422463299e76a248b1bb92317365ded42aa2cb90615f04bffb9e62faf0`, updated 2026-05-27. This is **upstream metadata**, not a locally verified hash or immutable license receipt. No tensor metadata or model bytes were fetched. [Release page](https://github.com/ack00gar/FunGen-AI-Powered-Funscript-Generator/releases/tag/models-v1.1.0).

Release tag `models-v1.1.0` resolved to commit `c95fd46f4b75925dab9da04acb4bb7a4704fffad`. Its [license](https://github.com/ack00gar/FunGen-AI-Powered-Funscript-Generator/blob/c95fd46f4b75925dab9da04acb4bb7a4704fffad/LICENSE) permits personal noncommercial use and prohibits redistribution without explicit permission. Current inspected HEAD `a21707c1ce7e91a73014be1fe116d8f1680cc0b2` has [PolyForm Strict plus supplemental terms](https://github.com/ack00gar/FunGen-AI-Powered-Funscript-Generator/blob/a21707c1ce7e91a73014be1fe116d8f1680cc0b2/LICENSE), explicitly restricting redistribution/repackaging. The [supplement](https://github.com/ack00gar/FunGen-AI-Powered-Funscript-Generator/blob/a21707c1ce7e91a73014be1fe116d8f1680cc0b2/LICENSE-SUPPLEMENTAL) includes downloaded weights in Software and restricts extraction/conversion/repackaging/outside use; it says the version shipped with a build governs that build.

**Release disposition: BLOCKED/UNKNOWN, not redistributable by assumption.** Exact weight-specific rights and the terms accompanying that asset must be established before bundling or importing it for outside use. Do not infer permission from a public URL, ONNX format, a local copy, or license terms for a different historical asset. Existing local artifacts should be preserved; inspection does not authorize extracting protected assets or altering licensing mechanisms. Software pack tooling and authored fixture packs can proceed independently.

For other Ultralytics-derived weights, the [manufacturer licensing page](https://www.ultralytics.com/license) offers AGPL/enterprise arrangements, while the [export documentation](https://docs.ultralytics.com/modes/export) exposes batch, image dimensions, precision, opset, dynamic shape and NMS/end-to-end options. Those settings change compatibility. Pin exporter commit, exact weights and their license; no architecture name automatically licenses third-party fine-tunes or supplies anatomy labels. Generic detector weights also do not establish Pulsar's motion quality. [Pinned upstream software license](https://github.com/ultralytics/ultralytics/blob/9b790cf26ffce6809723873a2547160085329b92/LICENSE).

## ONNX Runtime compatibility and redistribution

ORT v1.24.1 resolved to `470ae16099a74fe05e31f2530489332c0525edb5`. Its [MIT license](https://github.com/microsoft/onnxruntime/blob/470ae16099a74fe05e31f2530489332c0525edb5/LICENSE) permits redistribution subject to retained notices. The [third-party notices](https://github.com/microsoft/onnxruntime/blob/470ae16099a74fe05e31f2530489332c0525edb5/ThirdPartyNotices.txt) contain separate dependency terms; package the notices corresponding to the actual distributed build and its dependency inventory. ORT's MIT license does not license a model, CUDA/cuDNN/TensorRT binaries, or all native dependencies by implication.

The pinned [C API header](https://github.com/microsoft/onnxruntime/blob/470ae16099a74fe05e31f2530489332c0525edb5/include/onnxruntime/core/session/onnxruntime_c_api.h) declares API version 24; `GetApi` returns null for an unsupported requested API. Pulsar's exact binding source is [ort rc.13](https://github.com/pykeio/ort/blob/002f41a8e175eac7f6695ff361d2e51a50874c48/src/lib.rs). A version string alone does not establish compatibility: validate API, CPU architecture, OS/libc baseline, model IR/opset/domains/types, and the actual isolated worker load/run. [ORT compatibility guidance](https://onnxruntime.ai/docs/reference/compatibility.html).

[ORT provider build documentation](https://onnxruntime.ai/docs/build/eps.html) explains separately loaded provider/shared libraries and native dependencies, warning against system-wide search-path mixing. [CUDA compatibility](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html) depends on ORT build, CUDA and cuDNN versions. [ORT graph partitioning](https://onnxruntime.ai/docs/reference/high-level-design.html) can assign nodes to multiple providers. Consequently provider availability/registration is not evidence of all-node GPU execution. Capture actual provider traces and fail closed on unqualified substitution; fallback requires a separately qualified profile/attempt.

ONNX can reference external tensor data via relative locations and offset/length metadata. [Owning ONNX external-data specification](https://onnx.ai/onnx/repo-docs/ExternalData.html). A model-file digest alone does not pin its external weights. First pack version should reject external-data models until the model parser, manifest, worker snapshot and confinement pin the full dependency closure; never resolve unlisted files from ambient directories.

## Proposed bounded manifest

The following is a Pulsar design recommendation, not an upstream standard or existing feature. Store a small strict JSON manifest; reject unknown/duplicate keys and unsupported versions. Hash the **exact manifest bytes** for identity/signature, avoiding JSON reserialization ambiguity. Separate these fields:

| Field group | Required contents |
| --- | --- |
| Identity | schema version, pack ID/version, exact engine/adapter compatibility, creation/source identities. |
| Assets | Unique normalized relative path, role (`model`, `runtime`, `profile`, `license`, `notice`, `fixture`), positive bounded byte length, lowercase SHA-256. No URLs or local absolute paths required for offline import. |
| Runtime | Exact ORT version/API24, target OS/architecture/libc baseline, actual provider/build variants, all bundled native dependency identities. Runtime is executable native code, not merely model data. |
| Model | Format/IR/opset/domains, no unsupported custom operators/external data, explicit ordered labels, FP32 input/output names/shapes/layout, exact preprocess/decoder and exporter provenance. |
| Profiles | Existing `DeclaredInferenceProfile` bytes or identified profile asset, bound model/runtime hashes and resource envelopes. CPU is the currently admissible provider. |
| Rights | License expression or `LicenseRef` with exact bundled terms, authors/source/version, permitted uses/redistribution state, notices, permission evidence identity if needed. User assertions do not grant official release rights. |
| Evidence | Isolated loading/known-answer/real-media/provider/resource receipt identities, scope and limitations. Never let manifest assertions self-promote to `qualified_profiles`. |
| Trust | Detached signature/trusted catalog identity with declared signer policy; distinguish trusted release, locally approved custom, unknown. SHA-256 verifies bytes, not publisher authority. |

Security design must distinguish accidental corruption from malicious replacement, rollback and mix-and-match. [TUF security principles](https://theupdateframework.io/docs/security/) are a primary reference for those threats; this does not require introducing an entire framework merely for a local fixture importer. Official authenticated updates still need a concrete trusted-key/revocation/rollback design. Unknown runtime signatures must never silently authorize native library loading.

## Implementation sequence

1. Add pure manifest validation and bounded streamed verification; preserve existing profile schema. Test malformed manifests, placeholder hashes, duplicate IDs/paths/keys, wrong sizes, unsupported role/target/API/provider, and profile/model/runtime mismatch. Report integrity, compatibility, trust, rights and qualification separately.
2. Import into a private staging directory using exclusive/no-follow files. Enforce file-count, per-file and aggregate bytes, archive traversal/absolute/drive/UNC paths, symlink/hardlink/device rejection, case-fold collisions and compressed-expansion bounds. No scripts, hooks, model code execution, network calls or environment mutation during inspection.
3. Verify every asset and the exact manifest before immutable content-addressed publication. Fsync staged files/metadata, atomically publish, and transactionally record installed pack. Crash/interruption must retain the previous active set; partial packs cannot appear usable. A repaired file requires a fresh verification receipt.
4. Keep install distinct from activation. Engine validates a trusted/explicitly approved runtime and starts a new confined process for compatibility KATs; never initialize an untrusted library in the importing engine. Snapshots and native dependency loading must pin the complete closure. A custom model may reuse an already approved runtime rather than supplying executable code.
5. Explicit activation binds pack/profile identities to new jobs only. Existing project history/candidates keep their original model/source identities. Retain old packs needed by active jobs; rollback changes selection without silently regenerating masters. Relocation resolves pack-local relative paths, not developer caches or hardcoded `/usr/lib` references.
6. CLI/GUI status describes installed/verified/compatible/custom/qualified distinctly. Core/Complete release manifests may advertise only capabilities actually bundled and working offline. Authored constant-output ONNX fixtures prove packaging/wiring, never trained-model perception quality or an offline assistant/input-family completion.

## Evidence gaps

- No model bytes were parsed or run here: FunGen input/output metadata, label order, opset, exact license accompaniment and local digest remain unverified. Current default asset has adverse redistribution evidence; obtain specific rights or select a compatible licensed alternative.
- No final perception/assistant model, per-vendor backend or trained-model benchmark has been selected under MODEL-01/02. Placeholder registry entries cannot be activated or distributed as real model packs.
- ORT pinned source terms/API do not identify the user's native binary provenance, full dependency closure or Windows/macOS release viability.
- Runtime/model pack import machinery can be completed without claiming signed release metadata, real-media quality, acceleration, reference speed/memory, all required Fast modalities, assistant capability or Core/Complete qualification. Those require actual artifacts and scoped runtime evidence.
