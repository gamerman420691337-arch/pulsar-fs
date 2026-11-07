# Linux assistant bundle candidate

2026-10-11. Component checks PASS; roadmap PARTIAL; qualification remains open.

## Delivered candidate

The assembler now accepts an optional fourth absolute argument: a schema-1 assistant declaration with exactly `runtime`, `model`, `runtime_license`, `model_license` and `schema_version`. Each entry has an absolute `path`, positive `byte_len` and lowercase SHA-256. Declarations are captured from a regular opened file within 16 KiB; asset hashing streams through a 64 KiB buffer and rejects observed changes. Runtime admission requires an executable x86-64 ELF without an interpreter or dynamic dependency entries. Copied bytes must match the declaration. Existing output directories remain refused.

The actual candidate is retained at `/home/brianklam/Projects/Funscripts/assets/pulsar-linux-assistant-candidate-2026-10-11`. Its twelve payload files total 980,579,148 bytes and include Pulsar, the dedicated worker, static FFmpeg/FFprobe with media source/license/configuration, a static llama.cpp assistant, Qwen3-0.6B Q8_0 and the selected license texts. The manifest records relative assistant paths, exact identities, `qualified: false` and required host approval. Assembly creates no approved policy. The three-argument media/worker assembly remains supported.

Use `pulsar assistant-setup` with the candidate's local runtime/model and an explicitly chosen new private policy, following [host setup](PULSAR_ASSISTANT_HOST_SETUP.md). The package does not silently install or activate a model. It contains an assistant candidate, not the required generation model packs or a finished Core/Complete edition.

## Static runtime and compatibility

[The build recipe](../scripts/build-linux-assistant-runtime.sh) uses clean llama.cpp revision `b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c`, static linking, CPU-only inference and disabled native/AVX compiler options. The owned systemd build completed in 3min49.004s, with 2G peak memory. Existing shared ccache was automatically detected and used; it was not disabled or removed.

The resulting 15,246,256-byte executable has SHA-256 `c37ba4aebfe35df414dbf0fb63b208b45904f27c0cf12ed27718682963d2b83e`. Independent ELF inspection shows no interpreter or dynamic dependency entries. However, linked host libraries produce an ELF requirement for x86-64-baseline, x86-64-v2 and x86-64-v3, with a Linux 6.1.0 ABI note. Conservative compile flags therefore do not prove older-CPU or older-OS support. This is a current-host candidate. The broader platform matrix and static transitive dependency/license audit remain open. Copying license texts is not release licensing qualification.

After compilation, the recipe's ELF guard was changed to inspect captured output instead of a pipe under `pipefail`. Compilation flags and output bytes are unchanged. A fake-CMake fixture supplying a dynamic executable verifies refusal by the final guard; it is explicitly a simulated post-build check, not another C++ build.

## Verification and limits

Two assistant-bundle tests pass with no skips in a private 2 GiB tmpfs and network namespace: independent payload hashing, relocation to a path containing spaces, copied CLI offline host setup, private policy creation without Engine state, and oversized declaration, wrong model pin and dynamic-runtime refusal. Two existing worker-bundle tests also pass. Ordinary-storage assembly completed separately; all twelve retained payload identities match both independent rehashing and the final tested manifest.

The pinned static runtime and model pass six authored proposal cases and seven refusal/deadline checks through the actual Pulsar worker role. These use synthetic contexts and private namespaces/RLIMIT; they do not prove Engine cgroup/pool/fence dispatch, application effects, real-media quality or statistical model selection. No user media or hardware was used for these checks.

All 372 previous source/config identities are unchanged except the assembler; two added scripts bring the source inventory to 374. Production Rust and supplier binaries retain the preceding [host-setup checkpoint](evidence/roadmap-2026-10-11/assistant-host-setup/checkpoint.json) identities. No Rust rebuild or large library rerun was needed for this script-only increment. Final architecture 376, source guards 36 and shell/Node syntax checks pass.

[Exact evidence](evidence/roadmap-2026-10-11/static-assistant-bundle/checkpoint.json) retains executed/final recipes, source snapshots, build/configuration and ELF logs, worker requests/replies, tests, supplier identities and retirement observations. Actual reviewable artifacts remain in the project assets directory; generated scratch is removed after custody checks. Model/media annotations, generation packs/defaults, full delivery/platform/offline workflows, ordinary storage reliability, Handy 2 Pro firmware/keys and physical qualification, and human/release acceptance remain open.
