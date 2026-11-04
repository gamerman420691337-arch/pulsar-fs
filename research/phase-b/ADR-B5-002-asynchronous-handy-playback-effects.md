# ADR B5-002: asynchronous Handy playback effects

Status: conditional coordinator implemented; engine authority, durable recovery, transport and physical qualification remain open.

SAFE-01–05 and CONTRACT-09 require revision/configuration/controller pinning, disarmed reconnect and rejection of stale work. Blocking a coordinator lock while a driver waits for a network response would prevent revocation and stop from retiring that request. The Handy coordinator therefore emits a sealed effect intent and accepts a separate completion callback; it performs no I/O.

Each pending command carries its complete admission and clock mapping. Single-use sequence/session tokens bind completion to the current effect. Stop and reconnect retire prior play callbacks. Clock age, domain, drift uncertainty and submillisecond quantization are validated before encoding normal-rate, nonlooping Handy v3 play. Qualification limits are mandatory verified engine facts, without universal defaults or client deserialization.

A play intent sets physical uncertainty before external dispatch. Even an acknowledged stop leaves `StopUnconfirmed` until qualified physical evidence exists. Core `disarm_before_dispatch` releases only an admission for which no physical effect has begun; it refuses an outstanding physical stop. No remote acknowledgment becomes qualification or physical stop evidence.

An engine adapter must verify authority at transport handoff and execute effects outside the state lock, retain unknown outcomes durably, and verify driver ordering/cancellation/watchdog behavior. Ignoring late callbacks protects host state but cannot retract a request already received by a remote server. Real-media synchronization, exact Handy firmware/profile/driver qualification and physical stopping remain separate acceptance obligations. The module does not enable public physical playback commands.

See [implementation contract and evidence](../../docs/PULSAR_HANDY_PLAYBACK_COORDINATOR.md).
