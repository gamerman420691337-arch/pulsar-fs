# Annotated file generation on CUDA

Status: implemented and verified through an authored native file-generation/portable campaign. Roadmap PARTIAL; output remains unqualified. Campaign started October 8 and completed October 9, 2026.

Source-bound annotated temporal profiles now accept CPU or explicitly resource-declared version-2 CUDA. The exact RGB frame-pair recipe, model/runtime identities, source declaration and fixed tensor contract remain checked. Ordinary worker admission remains CPU-only: CUDA execution still requires the existing owner policy, supervised dependency capture, device binding, resource reservation, verified private bootstrap and original provider trace. No automatic provider substitution is introduced.

The temporal adapter, immutable attempt reader and annotated receipt verifier share request/profile correspondence checks. This metadata operation grants no launch authority. Portable export/import now permits annotated model bytes and CUDA execution evidence together, retaining both independent payload roles, immutable request binding, general receipt and ordered CUDA receipt/trace lineage. Missing roles, reordered lineage, changed bindings, unsupported providers and format downgrade still refuse.

## Native evidence

The [checkpoint](evidence/roadmap-2026-10-08/annotated-cuda-generation/checkpoint.json) records a shipped CLI → authenticated Engine → supervised CUDA worker campaign using an authored convolution model and a 28-frame stereo fixture. A declared right-eye integer region selects the exact reference pixels; source PTS starts at two seconds. A repeated native campaign explicitly checks every sample’s source PTS and elapsed project time, including zero-based 125 ms project keys. The campaign performs 27 model runs and retains 324 computation nodes, all reporting `CUDAExecutionProvider`. Fourteen authored reference pairs match CPU positions within a maximum absolute difference of `2.9802322387695312e-8`. This is numerical correspondence, not learned quality, tracking accuracy, or statistical qualification.

Both annotated-attempt and CUDA-candidate memberships persist. Restart preserves original receipt/trace bytes. Portable export/import creates a new clone whose standard export equals the original byte-for-byte, and another restart reopens the clone. A preceding reader refuses a private copy of the combined state without changing its database. Annotated CUDA Preview remains explicitly refused without adding CUDA completion membership. The separate native CPU annotated generation/selected-pack Preview/authority/pair-support campaign also passes.

The initial real convolution run exposed an incomplete captured dependency set: only the cuDNN shim and graph library were declared. A two-frame reproduction retained the same `CUDNN_STATUS_SUBLIBRARY_VERSION_MISMATCH`. Adding only `ops` moved the failure to convolution graph construction. Adding the remaining installed cuDNN sublibraries to a separate private owner policy allowed the unchanged model and loader to complete. NVIDIA's [documented full cuDNN configuration](https://docs.nvidia.com/deeplearning/cudnn/backend/v9.2.1/developer/misc.html) provides the dependency-configuration background; the actual matching-runtime result comes from this retained native campaign. Shared owner policies, system libraries and confinement were not changed, and no undeclared host-library fallback was added. The packet retains the failing policies, two-frame probes and intermediate portable/fixture failures alongside the passing reruns.

## Verification and reproduction

Final checks pass 1,124 workspace tests (63 ignored), 600 CUDA-feature Engine tests (68 ignored), both default and CUDA builds with existing warnings, 288 architecture checks and 36 source guards. The source archive binds 405 records: 392 preceding paths unchanged, nine Rust paths changed, one native JavaScript campaign added and three existing auxiliary research files newly captured by inventory audit. The latter files are unchanged and were not executed. Archive and evidence identities are verified by readback. Private campaign processes are terminal and worker custody is empty. Raw databases and credentials are excluded; authored model/RGB/reference fixtures are retained.

Run the native test from the repository root with a CUDA-enabled executable and an explicitly prepared private owner policy containing the matching dependency closure:

```sh
PULSAR_ANNOTATED_CUDA_BINARY=/absolute/path/to/pulsar \
PULSAR_ANNOTATED_CUDA_POLICY=/absolute/path/to/private-policy.json \
PULSAR_ANNOTATED_MODEL_FIXTURE_DIR=/absolute/path/to/fixture \
PULSAR_ANNOTATED_CUDA_OLD_READER=/absolute/path/to/preceding-pulsar \
node --test scripts/annotated-cuda-generation.test.mjs
```

Without the first three inputs the native test is skipped; a skip does not satisfy the gate. The optional old-reader input is required to reproduce the refusal check. `PULSAR_ANNOTATED_CUDA_EVIDENCE` can name a new destination for sanitized native outputs. Library identities are checked before execution; policy files must retain owner-private permissions. No dependency installation or runtime fallback occurs.

## Remaining obligations

Annotated CUDA Preview must handle absent/unsupported pairs without inventing model runs or weakening trace requirements. Annotated CUDA Stash batches still refuse; their dispatch, durable restart and publication paths need native coverage. Selected annotated CUDA model packs and desktop flows require their own verification. Sustained CUDA latency, acquisition faults, mixed-clock piecewise publication, useful held-out model quality and real annotations, installed Stash, broader fault/platform/offline/vendor qualification, physical Handy and human/release acceptance remain open. No fitting, live/sterile input, user Stash contact or physical playback occurred in this campaign.

The subsequent [actual Linux desktop CUDA generation campaign](PULSAR_DESKTOP_CUDA_GENERATION.md) verifies the authored file-model generation, candidate commit, export, Undo/Redo and restart workflow. Selected-pack/placed desktop combinations and sustained performance remain separate open gates.
