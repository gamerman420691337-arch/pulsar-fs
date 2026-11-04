# Assistant tool surface: MCP server, movement API and funscript authoring

Status: design recorded 2026-11-04. **Not implemented.** Nothing in this document
exists as code yet; the Engine protocol, the CLI and the loopback browser server do.

## What already exists

There is **no MCP server** in Pulsar, and no HTTP API for motion. What exists is a
typed, authenticated IPC contract:

| Surface | Shape | Reachable by |
| --- | --- | --- |
| Engine protocol | ~200 typed commands over an authenticated Unix socket | first-party clients only |
| CLI | `pulsar <command> ...` | operator |
| Browser loopback server | HTTP, token-scoped, preview only | the browser player it launched |

Motion and script authoring are already first-class Engine operations:
`PrepareHandyPlayback`, `StartHandyPlayback`, `PlayHandyHspLive`,
`StopHandyHspLive`, `HandyPlaybackStatus`, `CancelHandyPlayback`, plus the project,
candidate, commit and export commands. The neutral master is a standard `.funscript`
action list (`at` in milliseconds, `pos` 0-100), and export/commit already preserve
protected regions and provenance.

So the substrate for "easy API for movement and funscripts" exists. What does not
exist is an **external, model-facing** surface, and that omission is deliberate:
EDIT-05 states that media text and model output are data, never authority to operate
devices.

## What this document proposes

A local **MCP server** (`pulsar-mcp`) that is a thin adapter over the existing Engine
client, exposing the broker's allowlist as MCP tools. It adds no new authority: every
tool is an existing Engine command, and every effect-class tool still needs a human
approval that the MCP server cannot grant.

The MCP server runs as a user-launched local process with the user's own credential,
exactly like the CLI. A remote roleplay provider reaches it only if the user points
their client at it, and that is a CLD-02 disclosure decision, not an API feature.

## Tool contract

Three classes, mirroring ADR 0012. `stop` is never gated.

### Read

| Tool | Maps to | Returns |
| --- | --- | --- |
| `pulsar_project` | project/revision read | current project, revision, axes, protection summary |
| `pulsar_device_status` | `HandyPlaybackStatus`, `HandySessions` | connection and playback observation, never a liveness claim |
| `pulsar_review_flags` | review explanations | recorded flags with reason and confidence |

### Author — produces a candidate, never an effect

| Tool | Maps to | Effect |
| --- | --- | --- |
| `pulsar_propose_script` | pattern proposal / creative approval | candidate actions; no commit, no device |
| `pulsar_propose_edit` | `AssistantEditRecipes` → `ApplyAssistantEdit` | candidate with protected segments retained |
| `pulsar_commit` | candidate review/commit | requires the ordinary commit action and its approval |

`pulsar_propose_script` accepts a bounded action list — `[{at_ms, pos}]` with `pos` in
0-100 — which is the funscript contract the app already exports. It does not accept a
path, a URL or an executable.

### Effect — allowed while the user's control toggle is on

There is no per-call approval. The user turns on **AI control** once; the assistant then
moves the device directly inside the limits fixed at that moment.

| Tool | Maps to | Behaviour |
| --- | --- | --- |
| `pulsar_motion` | `PlayHandyHspLive` stream / `StartHandyPlayback` | submit motion now; refused when no session is active |
| `pulsar_stop` | `StopHandyHspLive`, `CancelHandyPlayback` | **always allowed**, even with no session and mid-stream |
| `pulsar_status` | `HandyPlaybackStatus`, session state | device, session, expiry and active limits |
| `pulsar_present_media` | player surface | same session gate as motion |

`pulsar_motion` takes the same bounded action list the app already exports — `[{at_ms,
pos}]` with `pos` in 0-100, or a single `{pos, duration_ms}` for immediate movement. It
accepts no path, URL or executable, and the Engine limits it to the session caps rather
than trusting the request.

The tool surface is deliberately small. A model that has to choose between six tools to
say "move" will use them badly; four tools with one way to move is the abstraction.

## The session the toggle creates

| Property | Rule |
| --- | --- |
| Created by | the user, from a single visible control |
| Lifetime | bounded (default 20 minutes), expires without action |
| Limits | device, travel range, amplitude cap, maximum rate, content class — fixed at creation |
| Visibility | expiry and state always on screen while active |
| Ended by | the toggle, stop, device loss, Engine restart, or expiry |
| Never | created, renewed or extended by a tool, model, or remote provider |

While a session is on, a manipulated model can move the device without a further prompt.
That is the accepted trade: the moment of informed consent is opening the session, not
confirming each stroke. The controls that make it defensible are the lifetime bound, the
enforced caps, the always-available stop, user-only creation and no persistence.

## Non-goals and refusals

- No shell, filesystem path, URL fetch or credential access from any tool argument.
- No settings mutation outside a tool's declared fields.
- No protection unlock, export/import, publication or device registration.
- No automatic device discovery or connection; registration remains an owner action.
- Tools return refusals as data. A refusal is never a silent retry with different
  arguments.

## Why an MCP server rather than a bespoke HTTP API

MCP is the shape local model clients already speak, so the same Toolset serves a local
roleplay model, a cloud client the user points at it, and any future desktop panel.
It also keeps the safety boundary in one place: the allowlist is enforced by the
broker, and the MCP layer cannot widen it.

A bespoke HTTP API would duplicate the same surface with a new authentication story,
which is more to qualify and more to get wrong.

## Work required

1. Engine-side session gate and consent enforcement (shared with the broker).
2. Device-control session state: bounded lifetime, fixed caps, disarm on loss.
3. `pulsar-mcp` crate: stdio MCP server over the existing `SessionClient`.
4. Tool schemas and refusal encoding; no free-form argument passthrough.
5. Desktop control: the toggle, the persistent stop, and visible expiry/limits.
6. Native tests for expiry, stop-mid-stream, session loss, capped motion and
   refusal when no session is active.

## Unqualified

No MCP client has been connected, no tool has been dispatched, and no device has been
actuated through this surface. Physical timing, stop behaviour and device response
remain under M5-DEVICE and M5-CONDUCT and are unqualified.
