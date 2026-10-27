# ADR B5-003: explicit live presentation correspondence

Status: implemented local mapping and captured-pair adapter; continuous and physical qualification open.

Finite captured-pair output previously used a new project origin for every pair. Live presentation requires a stable source/project/presentation correspondence, while original source clocks, capture age and optional buffering must remain distinguishable.

Use a pure, immutable unit-rate clock declaration with exact source and project anchors, an epoch-bound monotonic presentation anchor and a buffer bounded to one second. Preserve rational deltas and quantization remainders; reject unrepresentable results and ambiguous project ordering. Buffering changes presentation due time, never project action times or measured capture age. A clock-bound latest-frame scheduler seals mapped correspondence in its work token and checks every frame before mutation. Changed clocks or buffer policy require a new admitted epoch.

The production captured-pair worker consumes the optional declaration through the existing source, profile and authority boundaries and records mapped samples separately from source interval quantization. This functional primitive continues to use finite jobs. Declarations are not calibration receipts, native acquisition evidence, executable presentation buffering or permission to dispatch a device.

Measured host/player clock intake, continuous engine integration, protected byte ownership, actual buffering and physical control remain required successors. The selected Handy target and GPU fastest-preset six-axis release requirements are retained.
