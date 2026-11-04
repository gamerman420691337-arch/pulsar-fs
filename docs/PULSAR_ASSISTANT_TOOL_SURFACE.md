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

### Effect — requires human approval, per call

| Tool | Maps to | Approval |
| --- | --- | --- |
| `pulsar_play_script` | `PrepareHandyPlayback` → `StartHandyPlayback` | returns `approval_required` with a pending id; the user confirms in the app |
| `pulsar_play_live` | `PlayHandyHspLive` | same |
| `pulsar_stop` | `StopHandyHspLive`, `CancelHandyPlayback` | **never gated** |
| `pulsar_present_media` | player surface | same as play |

The MCP server **must not** be able to satisfy its own approval. A pending id is
single-use, digest-bound to the exact actions and revision, expires, and dies on
Engine restart. A model that replays an old pending id gets a refusal, not motion.

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

1. Engine-side consent and approval enforcement (needed by both this and the broker).
2. `pulsar-mcp` crate: stdio MCP server over the existing `SessionClient`.
3. Pending-approval store with single-use, digest-bound ids and app confirmation.
4. Tool schemas and refusal encoding; no free-form argument passthrough.
5. Desktop panel surfacing pending approvals and the active toolset.
6. Native tests for refusals, expiry, replay and stop-during-pending.

## Unqualified

No MCP client has been connected, no tool has been dispatched, and no device has been
actuated through this surface. Physical timing, stop behaviour and device response
remain under M5-DEVICE and M5-CONDUCT and are unqualified.
