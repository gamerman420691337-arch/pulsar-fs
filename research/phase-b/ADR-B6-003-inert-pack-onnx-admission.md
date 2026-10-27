# ADR B6-003: inspect ONNX dependencies and tensor declarations before inert pack publication

Status: implemented locally; model/native/platform/release qualification remains open.

## Decision

Hash identity does not establish an admitted dependency closure or inference ABI. Linux inert pack import/status therefore apply a bounded streaming structural ONNX check to the same descriptor whose bytes match the pack identity. Parsed profile bytes independently match their declared identity. External tensors, hidden external data in graph/attribute/sparse containers, unsupported function/training/device/custom-domain bodies and unknown structural fields refuse. Named float32 inputs/outputs bind the existing profile dimensions and decoder declarations.

The parser uses the [official ONNX protobuf schema](https://github.com/onnx/onnx/blob/main/onnx/onnx.proto3), inspected on 2026-10-04. Its subset and bounds are explicit in [offline pack documentation](../../docs/PULSAR_OFFLINE_MODEL_PACKS.md). It does not load supplier native code, verify rights/trust, prove operator/payload semantics or establish provider/model quality. Packs remain inactive and unqualified.

## Compatibility and recovery

No public protocol, database or pack receipt schema changes. Formerly admitted arbitrary model bytes cannot pass new status checks; those files are preserved. Historical mutation replay remains historical. New imports validate private captured bytes before atomic no-clobber publication and ordinary rejection removes staging entries. Unsupported features need an explicit future dependency/ABI adapter, not a silent bypass.

## Evidence

The [checkpoint](../../docs/evidence/roadmap-2026-10-01/pack-onnx-admission/validation.json) binds source, local tests and native shipped CLI import/status scenarios. No real external model/media/device was contacted or activated. Full semantic checks, publisher trust/revocation, rights, activation, bundles and all platform/clean-machine/release gates remain required.
