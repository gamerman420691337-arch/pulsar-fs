# Handy 2 Pro / FW4: public RPC contract refresh

Research date: 2026-10-09. Owner selected **Handy 2 Pro**; observed firmware and encrypted credential environment names remain unavailable. This research contacted only public documentation/source endpoints. No device discovery, connection, motion, authenticated API call, package installation, clone, or credential access occurred.

## Finding

A manufacturer-owned **public RPC Protobuf schema and generated serializers are available**. This is new evidence beyond the 2026-10-01 note. It supports an offline serialization/response-correlation implementation seam, including explicit HSP stop messages. It does **not** establish the FW4 BLE session/security/GATT framing contract or physical stopping behavior. Production offline playback must retain those gates.

The manufacturer repository is [handy-public-rpc at commit 7158570563bdcd6de356fff62e3f8c159189a6d2](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/tree/7158570563bdcd6de356fff62e3f8c159189a6d2). The unauthenticated GitLab API reported this HEAD, titled `CI/CD Update folder from original repo`, committed 2026-10-09 08:59:16 UTC. Its [README](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/7158570563bdcd6de356fff62e3f8c159189a6d2/README.md) labels the protocol a first draft unsuitable for use at its current stage. This is a maturity/compatibility constraint, not an instruction forbidding offline source-based tests.

## Retrieved source identities

SHA-256 hashes identify retrieved bytes; no source files were vendored by this research.

| Pinned path / artifact | SHA-256 |
| --- | --- |
| `public/proto/handy_rpc.proto` | `f36cb6109bc33d1eba5c9a3a06fed05c506a45328f48a1aeb886d9c794505917` |
| `public/proto/messages.proto` | `c3dc7a64c68bac16dbb51fbb3e0a47939ef119d2d259df2e8147f8c1347c05f5` |
| `public/proto/constants.proto` | `69cdda203e05720498833db23526c6a71211cac6784d607b682f2724615f4263` |
| `public/proto/notifications.proto` | `58a8adbc953684823a3d91b99b2f4607e35d183ec72f079490e6842d97fd0120` |
| `public/es6/bundle.js` (2,955,986 bytes) | `ead181d9e1d68afd77f90b118e64ec50a9ea3e43df098c49333d4c5806918f3e` |
| `public/es6/bundle.json` (130,070 bytes) | `b95ef4829393325fe706a7238acec4c6b8e6e1fa9b1b2bc94a9cc7a9e61978c0` |
| [REST v3 OpenAPI](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml), mutable URL | `9ad90e385ff1bf5f454a9af5f02862ca6cb064654b30df873d7d7389198cb684` |

The REST v3 bytes were re-fetched unauthenticated and match the previous research hash. The official [SDK page](https://ohdoki.notion.site/SDK-v2-2f77eee0c66d80f9bb8de2ba4ebdeb6d) still links `sweettechas/platform/handyfeeling-utils`; its observed HEAD remains `190784911d0ef920a38decb52e1cf5594be6338e`. Enumerating all 278 blob paths at that commit found no BLE/RPC/Protobuf-named source path; that limited inventory is not proof that the manufacturer has no other implementation.

## Application RPC envelope and correlation

[handy_rpc.proto](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/7158570563bdcd6de356fff62e3f8c159189a6d2/public/proto/handy_rpc.proto#L60) defines:

- `RpcMessage`: enum `type` field 1, oneof `request` 2 / `requests` 3 / `response` 4 / `notification` 5. Types are unknown 0, request 1, requests 2, response 3, notification 4.
- `Request`: `uint32 id` field 2; selected command is the `params` oneof. `Requests` bundles repeated requests at field 1.
- `Response`: `uint32 id` field 1; typed result oneof plus `Error error` field 2. Error contains code/message/data. Responses correspond to request IDs and are returned individually, even for a bundle. Notification ID field 2 is annotated FW4.0.14.

The schema comments distinguish immediate command acknowledgment from later operation-completion notifications. Neither is physical motion measurement. Treat unknown result variants, mismatched IDs, duplicate responses, wrong outer type, and ambiguous error/results explicitly; Protobuf decoding alone cannot enforce Pulsar admission or stop authority.

**This is the application-message envelope.** No retrieved owning source specifies an additional BLE header, length prefix, characteristic write/notify properties, fragmentation/reassembly, encryption negotiation, authentication handshake, retry/replay rules, or maximum complete RPC message size. Do not append inferred framing or reuse the legacy Protocomm session.

## Motion, script, and explicit stop roles

[Request command assignments](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/7158570563bdcd6de356fff62e3f8c159189a6d2/public/proto/handy_rpc.proto#L76) include current-mode stop 715 (annotated FW4.0.13), HAMP stop 721, HDSP stop 746, HSP setup/add/flush/play/stop 860/861/862/863/864, pause/resume 865/866, state 867. Flush and stop are distinct messages. There is no separately named HSSP request in this public RPC schema.

[HSP messages](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/7158570563bdcd6de356fff62e3f8c159189a6d2/public/proto/messages.proto#L759) carry setup stream ID; add repeated points, flush flag, tail index/threshold; play signed start time, optional estimated server time, positive playback rate, loop, and starvation-pause option. Stop is an empty request with a state-carrying response. Current-mode/HDSP stop are also empty requests. Session IDs report boot/socket/mode generations, but no BLE-specific session identifier is promised. Capabilities add BLE MTU in FW4.2; that field alone does not define packet splitting.

[Point/HspState](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/7158570563bdcd6de356fff62e3f8c159189a6d2/public/proto/constants.proto#L156) use point `uint32 t` and `uint32 x`; the current comment corrects an older position claim: 0–100 percent, with higher values clamped by firmware. State includes play status, capacity, current point/time, stream/tail indices, rate, loop and starvation policy. Reject out-of-profile coordinates in Pulsar rather than relying on firmware clamping. The point time unit is not annotated there; avoid silently inferring the complete timing contract from the field name.

The [REST v3 specification](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml) separately defines HSSP URL setup and `/hssp/stop`, versus HSP point buffering and `/hsp/stop`. Those HTTP endpoints are not BLE wire instructions. A full downloaded script may outlive its host process; no retrieved statement establishes a host-death stop deadline.

## Security, offline modes, and failure stopping

The [current official BLE guide](https://ohdoki.notion.site/Bluetooth-control-1ee344f4b55d42e5be14d98b001ee504) still gives service `77834d26-40f7-11ee-be56-0242ac120002`, TX `77835032-40f7-11ee-be56-0242ac120002`, RX `77835410-40f7-11ee-be56-0242ac120002`, and `ohd_hwX_*` advertising. It describes separate RX/TX and expanded RPC support, without binding the pinned draft schema to a specific Handy 2 Pro firmware release or specifying handshake/security framing.

The [manufacturer Onboarding guide](https://intercom.help/ohdoki/en/articles/9457608-how-to-use-the-handy-onboarding-app) documents Bluetooth control of HAMP/HRPP. Its standalone Offline boot mode prevents app discovery and leaves button control. Thus standalone offline capability, BLE pattern control, and arbitrary offline script streaming are separate claims. It documents no third-party script wire contract or engine-death watchdog.

[Handy 2 button documentation](https://intercom.help/ohdoki/en/articles/12161303-using-the-handy-2-buttons) describes the ON button as starting/stopping movement across modes. It provides no measured stopping bound or external-controller-loss guarantee.

[NotificationIdleTimeout](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/7158570563bdcd6de356fff62e3f8c159189a6d2/public/proto/notifications.proto#L139) is tied to motor inactivity, followed by a 60-second sleep warning; battery devices shut down and others disconnect Wi-Fi. This is **not evidence of an active-motion watchdog on host death**. No physical stopping deadline, disconnect behavior, queue cancellation guarantee, or authentication/replay mechanism was retrieved. These remain UNKNOWN, rather than proven absent.

The [legacy FW3 BLE guide](https://ohdoki.notion.site/Bluetooth-control-FW3-11d33fbce0474330b80d3b47beef838c) discourages new implementations. Its [pinned client README](https://gitlab.com/sweettechas/handy/handy-ble-client/-/blob/77c6479268f12808db59a56902a89d7f1b925375/README.md) uses Protocomm Security0 and ping-based connection reset. That evidence does not establish FW4 security or stopped motion.

## Next implementable seam: offline RPC codec, not a BLE writer

Use the pinned generated [ES6 serializer](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/7158570563bdcd6de356fff62e3f8c159189a6d2/public/es6/bundle.js#L11184) and generated JSON descriptor as an independent encoding/decoding oracle. ES6 imports `protobufjs/minimal`; generated C also exists under `public/proto-c/`. These artifacts are serializers, **not published device captures or golden test fixtures**. No serializer was executed in this research. The pinned repository tree has no LICENSE file, and the README provides no license grant; redistribution licensing remains UNKNOWN. Inspect terms before vendoring artifacts.

Small first cases are ID 1, current-mode stop and HSP stop, followed by bounded HSP-state response decoding/correlation. The generated encoder emits Request ID first, then command, and outer type then request. Source-derived candidate vectors are:

| Message object | Candidate bytes (unexecuted) |
| --- | --- |
| `{type:1, request:{id:1, requestStopCurrentMode:{}}}` | `080112051001da2c00` |
| `{type:1, request:{id:1, requestHspStop:{}}}` | `080112051001823600` |

Verify these with the actual pinned generated encoder before calling them known-answer fixtures. Include independent decode and malformed/truncated/overflow/duplicate-oneof/error-correlation controls. Proto3 scalar defaults and generated explicit-property encoding can produce distinct valid bytes, so define the intended canonical request subset rather than requiring arbitrary messages to have unique bytes. Generated `encode` does not implicitly validate messages; Pulsar must enforce its own bounds.

An offline codec may record an RPC acknowledgment as requested/acknowledged, never as physical stopped evidence. Keep hardware writes unavailable until firmware identity, owning transport/session/security contract, explicit stop procedure, disconnect/engine-death behavior, and measured human-accepted stopping thresholds are supplied and tested. This research does not qualify hardware or complete the roadmap.

## Retrieval limits

Public GitLab API source retrieval succeeded, including pinned schema/generated artifacts and manufacturer group inventory. The manufacturer Bluetooth video link could not be rendered by the web tool (`Unsupported content-type: video/mp4`); no video transcript evidence was used. Search results for unrelated BLE-RPC libraries and user-suggestion boards were not adopted as contract evidence. Only the note above was created; no scratch/build tree requires cleanup.
