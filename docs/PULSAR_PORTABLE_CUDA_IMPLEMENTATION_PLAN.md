# Portable accepted CUDA evidence implementation

Status: software integrated; main Rust/native/feature gates pass, application qualification PARTIAL. Production CUDA admission remains disabled. See [current status](PULSAR_PORTABLE_CUDA_EVIDENCE.md).

The preceding capture entry point refused accepted CUDA projects. The integrated implementation replaces that refusal with checked membership, explicit byte roles, format propagation, clone retention and conditional reader barriers. The requirements below remain the verification contract.

## Required behavior

- Retain the original accepted candidate, attempt, canonical request digest, dedicated CUDA receipt identity and original trace identity as inert namespace facts.
- Bundle exact original receipt and trace bytes with explicit distinct object roles. Enforce the existing 64-KiB receipt and 8-MiB trace limits, aggregate package admission, hashing and cancellation.
- Validate accepted membership against the original job/attempt/candidate graph and the checked original request. Reuse `cuda_result::verify_bytes` for byte correspondence; it grants no launch authority and performs no original-path resolution.
- Preserve general worker receipt selection and lineage order. Current CUDA completion appends the dedicated receipt and trace after the general worker receipt, while package capture currently uses `candidate.lineage.last()` as the general receipt. The new role mapping must handle both CPU and CUDA lineage without heuristic artifact selection.
- Retain original namespace facts across clone, restart, re-export and a second clone. Imported jobs remain inert; imported evidence never creates current CUDA intent, device grants, queue admission or physical replay.
- Preserve legacy CPU archive bytes and meanings. New CUDA role mapping needs an explicit new package format, including propagation through nested original manifests, and a conditional destination reader barrier before any imported facts can be silently omitted by an old writer.
- Verify old-reader refusal before writable database/WAL initialization, including nonempty WAL preservation. Do not blanket-enable a new database version in existing validators without preserving each active CUDA intent/evidence and worker-fence invariant.

## Source owners and integration

1. Protocol shape and package validation: `crates/pulsar-protocol/src/project_package.rs`; origin graph bounds and format propagation: `project_package_origins.rs`.
2. Read-only accepted membership capture: `crates/pulsar-engine/src/cuda_acceptance.rs`, reusing its layout/capacity/member validation.
3. Capture and general-receipt/lineage selection: `project_packages.rs`. Its current selected general receipt must stop treating the CUDA trace as the general receipt.
4. Original-byte verification: `project_package_imports.rs::validate_evidence`, using the existing inert checked-worker-scope boundary and `cuda_result::verify_bytes`.
5. Original graph retention/projection: `project_package_import_plan.rs` and import storage. Existing clone logic clears active job origins and retains exact ancestor manifests; use those mechanisms rather than rewriting accepted evidence into a live destination attempt.
6. Conditional reader barrier and read-only startup: `authority.rs` plus CUDA intent, accepted membership and worker-fence scope validators. Active and archival state must stay distinct, including on hosts without Linux worker confinement.
7. Codec and native application integration: `project_package_format.rs`, package export/import and captured historical readers.

## Exit evidence

Protocol/codec corruption checks must reject missing or swapped CUDA roles, absent or mismatched original candidate/attempt membership, altered request digest, omitted/changed bytes and format downgrades. Native engine/CLI cases must exercise accepted evidence export, clone, restart, re-export, second clone and old-reader refusal while retaining CPU behavior. Authored accepted evidence proves consistency and custody only; actual engine-admitted GPU generation, trace authenticity, model/media performance and physical qualification remain separate gates.

The current registration-identity validation campaign runs against a frozen source subject. Begin production edits for this closure only after preserving its terminal results; do not relabel the previous full workspace/JavaScript campaign as evidence for the next implementation.
