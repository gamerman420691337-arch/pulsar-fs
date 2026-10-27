# Handy online inspection

The engine implements consent-bound, read-only Handy REST v3 inspection. The CLI shares this engine operation:

```sh
secrets run -- pulsar handy-inspect \
  --api-key-env HANDY_API_KEY \
  --connection-key-env HANDY_CONNECTION_KEY \
  --allow-online-inspection
```

Only environment variable names travel through IPC. The already-running engine must have both credential values in its environment; launching a client through `secrets run` does not update an existing engine's environment. Use the separately managed engine lifecycle to configure it. The application API key and device connection key are distinct credentials. No plaintext credential CLI option or configuration-file value is accepted.

The consent flag permits connectivity and identity queries only. The engine uses `https://www.handyfeeling.com/api/handy-rest/v3`, sends `X-Api-Key` and `X-Connection-Key`, and accepts no user-selected URL, proxy, redirect, upload, firmware change or motion command. Its separate private `handy-owner.token` must accompany an authenticated paired session. Stash and package owner capabilities do not authorize this operation. The owner capability is checked before credential loading/network activity and again before returning the report. It is not an API credential.

The operation issues `GET /connected`, followed by `GET /info` only if the connectivity response is true. A ten-second total host request deadline and 64 KiB response ceiling bound the operation; neither number is a physical stopping or acceptable playback-delay limit. There is no automatic retry, alternative API family or BLE/cloud fallback. HTTP/API errors and malformed data expose no remote message or response bytes. Credential-bearing identity text is withheld.

The report contains observed connectivity, optional exact firmware/model/variant/remote session identity, firmware status and a host observation timestamp. These sequential queries are not an atomic device snapshot or a promise of continuing connectivity. A reported firmware version is an observation, not a selected qualified driver configuration. Disconnect returns no identity. Remote update status does not trigger an update.

This operation cannot arm, play or stop a device, cannot construct a qualified profile, and sends no scripts, source media, prompts or provenance. Physical-effect commands remain unsupported. Actual playback needs a separate explicit effect consent, exact configuration qualification, revision-pinned admission, controller ownership, clock mapping/age checks and failure/stop evidence. See [Handy protocol research](../research/phase-b/handy-integration-research-2026-10-01.md) and [wire codecs](PULSAR_HANDY_WIRE_CODECS.md).

Validation uses synthetic loopback HTTP fixtures and isolated native engine/CLI state with deliberately absent credential variables. No actual Handy was contacted. The [inspection evidence receipt](evidence/roadmap-2026-10-01/handy-inspection/validation.json) records the completed gates and their scope. Physical, GUI, platform and release qualification remain open.

The later [desktop inspection workflow](PULSAR_DESKTOP_HANDY_INSPECTION.md) uses the same shared Client helper and Engine operation, with explicit per-request consent, background dispatch and checked timestamped observation display. Its separate headless/native-preflight evidence does not establish online connectivity or physical qualification.
