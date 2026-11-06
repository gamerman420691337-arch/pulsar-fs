# Authenticated capture-session lifecycle

Pulsar now exposes engine-owned capture lease limits, start, status and stop through the authenticated protocol and shipped CLI. A lease owns the bounded pixel runtime, a paired actor, an exact project revision and fresh source/capture identities within the current engine boot. Active means that this memory lease exists; it does not mean a camera is open, pixels are arriving, inference is running or a device is moving.

Starting requires project Read, ImportSource and Generate grants, the current expected revision, a matching engine boot nonce and validated clock/policy/lease options. Limits disclose the current nonce, a maximum of eight active leases, and a maximum lease lifetime of 30 seconds. One actor may hold one active capture lease per project. There is no native source path or URL in this API and no permission to access an unspecified camera or stream. The source version is a fresh ephemeral capture identity, not an imported media row or portable source artifact.

The server creates the capture epoch and adds it to the declared source/project/presentation correspondence. Producer clock anchors remain declarations. Reports include the admitted identity, options, clock, current elapsed server tick, lifetime state and reserved pixel bytes. The [authenticated frame channel](PULSAR_LIVE_FRAME_INGRESS.md) accepts fixed-size RGB frames and reports the latest header and received-frame count. Pixel admission is 61,440 bytes per active runtime; model, preprocessing, worker and acquisition buffers require their separate budgets.

A start is remembered transactionally under the exact request fingerprint in the existing private request ledger. If receipt insertion fails, no lease is published and resource admission is released. Repeating the same start returns its current lease state; it cannot reopen a stopped, expired or evicted lease. Reusing the request ID with different options refuses. After an Engine restart, [exact saved admission replay](PULSAR_CAPTURE_RESTART_RECEIPTS.md) can return inactive EngineRestarted metadata after current authorization/revision checks. A new admission with the old nonce still refuses, and no live registry is reconstructed from stored active-looking reports. The old source/capture identities never become current authority.

Status and stop require project Read and the original paired actor, project and admitted revision. They can report an already retired old-revision lease without accepting it into the new revision. Other paired actors cannot inspect or stop it merely because they can read the project. Stop retires the runtime and is replayable; it does not acknowledge a physical device stop. Revocation followed by restoration of Read does not reactivate a lease.

Revision changes, missing required grants and expiry retire the runtime and release its owned pixel admission. Requests check invalidation before/after normal dispatch, and a weak engine-owned maintenance thread checks leases every 250ms after the first capture is started. That nominal interval is subject to thread/DB scheduling and is not a qualified real-time guarantee. Status polling does not renew the absolute lease lifetime. The registry retains at most 64 reports and evicts the oldest inactive entries. Evicted start replay reports inactive, without fabricating a current elapsed clock. Registry metadata is not restored after boot.

The options file is explicit:

```json
{
  "policy": {"maximum_analysis_age_ns": 200000000, "added_buffer_ns": 0},
  "source_anchor": {"numerator": 0, "denominator": 1},
  "project_anchor": 0,
  "presentation_anchor": 0,
  "lease_ms": 30000
}
```

```sh
pulsar live-capture limits PROJECT_ID 0
pulsar live-capture start PROJECT_ID 0 capture-options.json --request-id START_ID
pulsar live-capture status PROJECT_ID 0 CAPTURE_EPOCH
pulsar live-capture stop PROJECT_ID 0 CAPTURE_EPOCH --request-id STOP_ID
```

The CLI obtains the current boot nonce from limits before normal start. [Exact historical replay](PULSAR_LIVE_CAPTURE_CLI_REPLAY.md) accepts the original `--engine-epoch` only with an explicit `--request-id` on Start and both continuation forms. An explicit request ID supports reconciliation; retrying it against a different boot or changed options cannot silently create another lease. Options are read through the bounded JSON admission helper. Responses validate clock/lifetime/pixel consistency and project correspondence in the client. `native_acquisition` and `physical_dispatch` remain false. `continuous_inference` discloses the implemented confined CPU adapter when its pinned runtime/platform is available; an explicit model/profile is still required and no qualification follows.

Native software coverage includes lifecycle replay without rearm, request conflict, active lease limits, exact revision preservation, actor isolation, revision invalidation, expiry, maintenance expiry without another client RPC, grant revocation/restoration, failed receipt rollback and restart refusal. Shipped CLI coverage crosses local IPC and independently exercises limits, start/status/stop, repeated requests and malformed options. These are authored software fixtures, not media or device qualification.

Required next work is acquisition adapters and measured timestamp provenance, renewal/continuity rules for actual running sources, broader continuous-source behavior and qualified GPU inference, projection/six-axis work, scheduling/buffering, source/player clock qualification and device admission/transport/response. The selected Handy target and GPU fastest-preset six-axis requirements remain unchanged. Camera/stream, physical, platform, model-quality, human effort and release gates remain open.

The historical lifecycle-only [validation checkpoint](evidence/roadmap-2026-10-01/live-capture-sessions/validation.json) binds its source and native binaries to 722 passing Rust tests, seven ignored qualification cases, all 74 passing native-configured JavaScript cases without skips and 169 passing structural architecture checks. Six engine lifecycle regressions, bounded protocol options, native CLI traces and raw gate logs are retained. Native build and diff checks passed. Failed verification attempts are retained: global tmpfs exhaustion, slower/contended storage startup deadlines, and a campaign staging failure caused by its minimal child environment dropping the selected TMPDIR. The supervisor now explicitly uses the admitted private campaign root for runner staging, forwards no unrelated environment and preserves existing deadlines. Final checks ran serially on home NVMe; no old fixture was deleted. These are local software gates, not media/device/release qualification.
