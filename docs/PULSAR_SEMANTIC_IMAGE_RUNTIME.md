# Semantic image runtime candidate

Status: **PARTIAL, unqualified, not integrated into Pulsar**.

The current still-image generation path validates and decodes the image but synthesizes motion from the constrained prompt alone. It explicitly records `image_not_semantically_conditioning_motion`. This increment supplies a real local pixel-conditioned runtime candidate and a repeatable experiment, not a replacement generation path.

## Native result

Pinned llama.cpp `b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c` built a generic x86-64 Linux static `llama-mtmd-cli`, with CPU-only inference and video/subprocess/HTTPS support disabled. Readelf found no interpreter or shared dependencies. Runtime SHA-256 is `c38a6afc4790b64e2cfc86e4de5a08f818793fa1cea44e3f9eb5df9f82a90601` (17,128,488 bytes).

The ggml-org conversion of publisher SmolVLM-256M-Instruct uses a verified Q8 text model and matching F16 projector. Their pinned revision, sizes and hashes are recorded in the checkpoint. Publisher and conversion provenance remain distinct; generic publisher benchmarks are not Pulsar quality evidence. See [primary-source research](../research/phase-b/semantic-image-runtime-research-2026-10-11.md).

Two confined native controls passed. With the same prompt, schema, seed and guest path `/image.png`, an authored red circle returned `{"shape":"circle","color":"red"}` in 47.26 seconds and an authored red square returned `{"shape":"square","color":"red"}` in 40.80 seconds. Both processes exited zero. The parser accepts only the entire JSON body, exact fields and finite enum values; it rejects surrounding logs and duplicate members. This proves these two simple pixel controls, not general semantics or useful motion.

The initial invocation failed before inference: `error: invalid argument: --simple-io`. This MTMD CLI has a different flag and output contract from the text completion CLI. The final invocation omits that flag; both failure and final logs are retained.

`build-linux-vision-runtime.sh` pins the clean source revision and static CPU baseline. `evaluate-local-vision.py` checks exact candidate identities, creates its own images, and invokes the runtime in `bwrap --unshare-all` with fixed read-only mounts. It inherits the existing bounded supervisor: 2 GiB address space, 120 seconds per request and 256 KiB per output pipe. These are experiment controls, not measured cgroup RAM or production aggregate resource admission.

## Remaining implementation

The production path needs explicit vision profile and projector/pixel identities; immutable capture, RAM/CPU admission and process custody; bounded child-only image parsing; source/preprocessing lineage and fresh approval bindings; constrained creative proposal validation; and separate inference/generation approvals. The experimental upstream CLI is not production-ready. A small library adapter can avoid CLI conventions. Static imagery may suggest creative motion, but cannot establish observed temporal motion or faithful six-axis reconstruction.

No source-matched annotations were supplied. No training or fabricated labels occurred. No user media, sterile/live references, device secrets or physical hardware were used. Candidates are retained privately under the project's assets directory, `qualified:false` and `selected_default:false`; distribution notices, quality, latency, cancellation, installation and physical/human gates remain open.

Evidence: [checkpoint](evidence/roadmap-2026-10-11/semantic-image-runtime/checkpoint.json).
