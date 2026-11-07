# ADR B3-082: untrusted local-assistant proposal foundation

Status: data contract implemented; native candidate evaluated and rejected for default selection; application integration pending. Full roadmap INCOMPLETE.

## Decision

Keep learned inference separate from host-authorized effects. Define bounded strict proposal JSON referencing host-issued target IDs, recorded review IDs, allowed sample strides and precomputed edit recipe IDs, plus NoAction. Resolve references only against validated context. No arbitrary paths, code, device actions, protection replacement or invented evidence enters this contract. Context checks do not prove current session/project/source/view/revision or interpret user intent; those checks and explicit approval belong to the future engine broker immediately before dispatch through existing effect/history interfaces.

Use real pinned learned-model CPU experiments, not a renamed deterministic text parser. Preserve every failed response. Two six-case synthetic campaigns with official Qwen2.5-0.5B Q4_K_M produced only one/two correct answers. Do not select this candidate as the release default based on valid JSON or a single successful target selection. Research tooling, address-space limits and the upstream completion trailer are not a shipped runtime, sandbox or application action ABI.

## Compatibility and verification

Only new exported protocol data types and development tooling; no existing Command/Response encoding, version, schema, dependencies or authority changes. Strict unknown-field refusal includes the empty NoAction variant. Protocol library 112 and contracts 10 pass; strict protocol Clippy, formatting, architecture 353 and guards 36 pass. Frozen fixtures remain an independent recorded check. Whole-application/quality/platform/device/installer/approval/undo/memory-handoff gates remain open. See [execution and evidence](../../docs/PULSAR_LOCAL_ASSISTANT_CANDIDATE.md).
