# Owner CUDA policy in the real engine

Engine startup now accepts an explicit private `PULSAR_CUDA_OWNER_POLICY` file, or the equivalent `EngineConfig::cuda_owner_policy` path. The default remains absent. This configures expected device/library identities and cooperative accounting; it does not enable GPU jobs, load those libraries or qualify a model/backend.

The schema-1 file has four fields: `schema_version`, `device` (kernel UUID/PCI/minor), `accounting` (logical ordinal and GPU reservation/interactive/control budgets) and `native_dependencies` (the existing expected-byte library specification). Kernel and accounting UUIDs must agree. Ordinal and budgets pass shared resource-pool validation. Library paths cannot contain parent traversal; aliases and expected bytes pass bounded specification validation. The current image requires loader, driver, ONNX runtime, CUDA/shared providers and cuDNN aliases. These required names do not establish complete arbitrary lazy-library closure or ABI compatibility.

The reader opens an absolute nofollow regular file, requires current-user ownership and no group/other permission bits, and reads at most 1 MiB. Unknown fields and malformed/bounded declarations refuse. The identity binds exact read bytes, including formatting. Loaded declarations remain fixed for the engine lifetime; changing or replacing the pathname requires a restart to apply new intent. This is a startup configuration interface, not a live revocation mechanism.

Before engine state opens, the policy's declared ONNX identity must match the engine's pinned runtime. The captured startup runtime is checked again before the engine is returned. Other declared library bytes are not captured or loaded by this step. A concurrent runtime change can therefore still fail the second check after startup storage has been created; the stable mismatch scenario is verified before any state creation.

The real engine constructs its shared RAM/job/storage/GPU pool using the policy's accounting inventory. Engine status checks actual shared GPU usage; merely loaded metadata has no pool-usage value. The normal authenticated inference catalog still advertises CPU only, with an explicit configured-policy/disabled-GPU explanation. No wire/database/profile schema changes occur.

CUDA generation/preview intent now checks owner presence, version-2 resources, requested UUID/ordinal, runtime identity and the applicable offline/interactive ceiling before resource reservation or attempt-file creation. Matching intent still refuses because captured GPU identities are not yet carried through durable worker attempts. Neither a private policy file nor a valid profile renews model, hardware or release authority.

## Evidence

The [final receipt](evidence/roadmap-2026-10-01/cuda-owner-policy/validation.json) records 879 passing Rust tests, zero failures and seven ignored qualification cases across 43 suites, native build, all 94 native-configured JavaScript cases without skips, structural/diff checks and exact source/binary/artifact readback. Three new unit cases cover policy identity/roles/priority, private bounded snapshot/refusal and invalid-policy rejection before state creation.

Eleven additional native scenarios open isolated real engines and use the shipped CLI. A valid private declaration produces the configured accounting inventory and actual zero shared GPU claims, with execution disabled. Six startup refusals cover runtime mismatch, unknown fields, a missing required alias, inconsistent UUIDs, group-readable storage and a symlink. Four authenticated CUDA-intent cases cover matching but unsupported intent, a foreign ordinal/runtime and an excessive offline claim. All four retain zero job/attempt rows and zero attempt directories; no output script is created.

The initial passing subject remains separate. Its matching intent reached an unpublished empty attempt directory before the old adapter refusal. Inspection reproduced one such directory. The final refusal was moved ahead of reservation/directory creation, and native assertions now check the filesystem as well as SQLite rows. The existing private fixture is retained rather than deleted or rewritten. Final Rust/native/JavaScript checks follow the correction.

Only selected metadata and test outputs enter the public evidence packet. Engine databases, pairing/bearer material and private fixture state remain outside it. No native CUDA execution, private media/training, physical Handy, installed Stash, native-window or release qualification is added by this increment. The preceding [strict Rust CUDA session evidence](PULSAR_RUST_CUDA_SESSIONS.md) remains its own subject.

## Remaining GPU path

Capture the approved dependency image under storage/lifetime supervision; combine fresh device observations with aggregate reservation and held kernel nodes. Bind the exact owner policy, image, resource claim and selected profile into durable attempts and worker transport. Connect the existing strict session, trace/result validation and process cancellation/restart fencing before removing the disabled-launch guard. Then qualify actual model/media performance and required platforms/devices under the full roadmap.
