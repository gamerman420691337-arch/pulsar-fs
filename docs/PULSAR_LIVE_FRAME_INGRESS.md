# Authenticated live RGB frame ingress

The successor [local clock observation helper](PULSAR_LIVE_CLOCK_OBSERVATIONS.md) now brackets this channel's authenticated handshake and supports conservative captured-event upload. Producer source/capture provenance remains declared; local IPC observations do not qualify acquisition or device/player synchronization.

A paired producer can now send packed 64x64 RGB frames into an active engine-owned capture lease through the existing owner-only local bulk endpoint. This connects authenticated source input to the bounded latest-pair runtime. It does not acquire a camera, run continuous inference, buffer a player or dispatch device commands.

The strict `live_capture` channel marker distinguishes the handshake from motion and package transfers. The handshake binds session/bearer authentication, engine boot, capture epoch, project, admitted revision and ephemeral source identity. Each frame header contains its ordinal, exact rational source timestamp, producer-declared lease-relative capture tick and SHA-256. There is no producer-selected pixel length, source path or URL. Project presentation time is derived from the admitted clock; the producer cannot replace it per frame.

For every header the engine rechecks authentication, Read/ImportSource/Generate grants, owner, scope and active lifetime before issuing `send_pixels`. Source and mapped project times must strictly increase; capture ticks cannot regress or exceed current engine elapsed time. The peer then sends exactly 12,288 bytes, without artifact chunk framing. After this bounded read, the engine repeats authorization and lease retirement checks before verifying and offering the bytes to its pixel runtime. A revocation, revision change, expiry or stop during the read refuses the effect. A connection is not permanent authority.

The channel shares the bulk server's limit of 16 connections and ten-second I/O budgets. One fixed 12,288-byte connection read buffer exists outside the separately reserved 61,440-byte retained pixel capacity. Header decoding remains bounded to 16 KiB. No per-frame files, database receipts or growing pixel queues are created. Pending work is replaced by the latest chronological pair; issued-work ownership and discontinuity rules remain those of the [pixel runtime](PULSAR_LIVE_PIXEL_OWNERSHIP.md). The [persistent worker primitive](PULSAR_PERSISTENT_LIVE_WORKER.md) can analyze issued work, and [authenticated capture sessions](PULSAR_AUTHENTICATED_LIVE_INFERENCE.md) now launch and feed it with explicit admission.

Capture reports retain only one latest frame header and a checked received-frame counter. An exact retry of that latest ordinal, metadata and verified bytes returns the existing receipt without incrementing the counter or replacing pixels. Changed metadata or bytes refuse; older ordinals cannot be replayed. This receipt is ephemeral and cannot restore a live session after engine restart. Original start receipts remain transactional and replay current capture status.

The protocol client preserves a connection after local validation errors, but poisons it after remote, I/O or response-identity errors. It never automatically resends pixels after an ambiguous outcome. Reconcile capture status and its latest header, then explicitly reconnect and retry the exact latest frame only while that lease remains active. Later frames supersede the bounded retry receipt.

The shipped CLI supports single-frame transport:

```sh
pulsar live-capture push PROJECT_ID REVISION CAPTURE_EPOCH frame.json frame.rgb
```

`frame.rgb` is exactly one packed RGB image. The header file is explicit:

```json
{
  "frame": 0,
  "source_time": {"numerator": 0, "denominator": 1},
  "capture_tick": 0,
  "sha256": "REPLACE_WITH_FRAME_SHA256"
}
```

The CLI reads only bounded regular files without following symlinks, verifies their byte count/hash, obtains current capture status and opens the authenticated channel. `SessionClient::open_live_bulk` also exposes a persistent channel to first-party producers. Capture ticks and source anchors remain declarations; comparing a tick to the server lease clock does not measure clock synchronization or capture latency. There is no lease renewal and the absolute thirty-second maximum remains in force.

Native regressions exercise frame replacement and exact pixels, retry/conflict behavior, hash/clock/scope rejection, authority changes between header and body, and absence of durable frame receipts. Authored Unix peers test two-phase framing, persistent retry, response-identity poisoning and refusal to send pixels before permission. The shipped CLI regression crosses the actual engine bulk endpoint. These fixtures qualify local software behavior only. Acquisition, qualified models, calibrated clocks, player buffering, GPU six-axis capability and Handy device qualification remain required.

The retained [validation checkpoint](evidence/roadmap-2026-10-01/live-frame-ingress/validation.json) binds source and binaries to 729 passing Rust tests, seven ignored qualification cases, all 74 native-configured JavaScript tests without skips and 170 structural checks. Native build and diff checks passed. Sixteen CLI capture traces, raw logs and authored input hashes are retained. Project database schema 7, portable format 6 and private Stash proof schemas 1/2 are unchanged. Training and the prior negative sterile evaluation were not changed.
