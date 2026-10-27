# ADR B5-009: Conservative local capture-clock probes

Status: implemented software increment; source/device/physical qualification open.

## Context

Authenticated leases and persistent CPU inference use producer-declared capture ticks. Acquisition adapters need a process-local mapping to the engine lease clock. Copying status time onto newly uploaded pixels loses acquisition delay, while assigning the midpoint of a request interval can understate age.

## Decision

Measure the existing authenticated bulk handshake with local monotonic instants. Its fresh engine clock sample lies inside the measured request interval. A pure checked interval kernel maps later acquisition events under the disclosed equal-rate local monotonic assumption. The client sends the earliest possible lease tick and retains both bounds, exact identity and uncertainty. Source rational PTS stays independent and unchanged.

Keep the runtime observation private and non-deserializable. Refuse pre-probe/future events, overflow, excess uncertainty and any interval reaching absolute lease expiry. Poison it with its connection on ambiguous exchanges. Slow handshakes do not prevent the existing declared-tick protocol, but they cannot silently produce a usable measured observation. No automatic clock refresh or rearm follows.

Provide a read-only CLI probe and a shared captured-upload helper. The engine continues independent grant/revision/epoch/age checks and treats capture provenance as declared. Client facts do not become engine or camera attestations, device calibration, qualified synchronization, motion or dispatch authority.

## Evidence and remaining work

See [local clock observations](../../docs/PULSAR_LIVE_CLOCK_OBSERVATIONS.md) for the contract and retained software evidence. Native same-host transport and actual engine CLI checks are separate from authored pure interval proofs. Camera/stream adapters, acquisition provenance, continuity/renewal, measured device/player rates, GPU six-axis and physical/human/release gates remain required. The model and negative sterile evaluation are unchanged.
