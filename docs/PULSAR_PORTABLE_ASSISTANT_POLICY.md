# Portable approved assistant policy

Verification: final policy tests, library regression and all-feature binary build PASS. Existing Linux host-policy scope; full roadmap INCOMPLETE.

Schema 2 keeps the existing private owner-policy fields and explicit `approve_inference: true`, but declares `runtime_path` and `model_path` relative to the policy file's parent, for example `assets/llama-completion` and `assets/model.gguf`. Move the policy and assets together, then select the moved absolute policy path through `PULSAR_ASSISTANT_OWNER_POLICY`. The asset declarations, hashes, worker identity, deadlines and resource limits stay unchanged. No current-directory lookup or download occurs.

Each relative path must contain only normal components and resolve without a symlink. Empty, absolute, parent-traversal, missing and symlinked assets refuse before Engine state creation. The existing private policy capture and actual runtime/model capture/hash checks remain authoritative. Schema 1 continues to require absolute asset paths; unknown versions and missing explicit approval refuse.

This implements portable policy resolution, not a complete portable edition. Core/Complete packaging, chosen bundled model/runtime, cross-platform execution, useful quality, ordinary-storage reliability, devices and human/release gates remain open. See [ADR B3-099](../research/phase-b/ADR-B3-099-portable-assistant-policy-assets.md).


Four policy tests pass on ordinary storage in 1.20 seconds, including relocation with unchanged declarations, legacy approval/metadata/resource checks and path/version refusals. The final default Engine library suite passes 763 tests, zero failures and 59 ignored in 30.16 seconds under a two-GiB private tmpfs. Default and all-feature strict workspace Clippy, formatting, 376 structural checks and 36 source guards pass. These results do not enable ignored native/device gates or close ordinary-storage reliability.

The final offline locked all-feature workspace binary build passes in 54.72 seconds. Assistant CLI help passes from the actual Cargo-reported executable path; this checks parser startup only. A first help invocation used an incorrect assumed output path and returned127; its output is retained separately. [Exact source/build/test evidence](evidence/roadmap-2026-10-11/assistant-relocation/checkpoint.json) preserves the source change and gate scope.


[First-party host setup](PULSAR_ASSISTANT_HOST_SETUP.md) now measures selected local files and writes the private schema-2 policy without hand-authored identities. Explicit host approval and later inference/effect approvals remain separate; actual offline editions are still required.
