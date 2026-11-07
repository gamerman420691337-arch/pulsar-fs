# ADR B3-144: desktop Handy observation

Status: implemented with scoped headless/native-preflight evidence. Online identity, rendered GUI and physical playback qualification remain open.

Expose the existing read-only Handy inspection in the Devices tab. Capture distinct credential environment-variable names and explicit per-request online consent. Dispatch on the existing bounded RPC worker; load the separate private owner proof only from the trusted session bootstrap directory. CLI and desktop share the same Client helper and Engine operation. The helper accepts only a validated Handy inspection response. Missing proof, invalid names, absent consent and unexpected/malformed responses refuse.

Disable controls while work is pending. Consume consent and clear the old observation when sending; changing names also clears observation/consent. No automatic polling, retry, credential provisioning, Engine restart, endpoint override or device effect. Display a timestamped connectivity/model/firmware snapshot without remote session ID. Connectivity does not admit motion or qualify the selected Handy 2 Pro.

Four headless tests exercise actual drawn checkbox/button input, pending/invalid states, captured names, consumed permission, report validation and off-thread helper dispatch. Full Clients pass 322 with 8 ignored. Two existing unchanged Engine authorization tests pass. The network-disabled native controller exercises the production helper against a fresh Engine with absent credential names and verifies unchanged project state; three actual CLI refusals pass. Both strict lint configurations, workspace check, formatting, native App/Worker/controller builds, 445 architecture checks and 36 source guards pass. No online success is inferred from these negative preflight controls.

See [desktop scope](../../docs/PULSAR_DESKTOP_HANDY_INSPECTION.md) and the [existing API contract](../../docs/PULSAR_HANDY_INSPECTION.md). Physical, model/media, platform/human and release gates remain independent.
