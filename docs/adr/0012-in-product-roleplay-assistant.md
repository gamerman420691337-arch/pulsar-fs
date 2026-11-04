# ADR 0012: In-product roleplay assistant with an allowlisted tool surface

Status: Accepted owner direction (2026-11-04). Implementation not started; the full
roadmap remains INCOMPLETE and no assistant, media, image or device capability
described here is qualified.

## Context

The confirmed assistant contract (EDIT-01 through EDIT-05) is deliberately narrow.
The bundled local assistant performs bounded application actions reached through
separate approvals: target selection, review-flag explanations, generation-setting
changes and undoable script edits. Model output is data. It carries no authority to
operate devices, disclose files or install models.

The product owner has directed that the assistant additionally become a
conversational, in-product **roleplay companion** able to:

1. hold a multi-turn roleplay conversation,
2. author funscripts (propose motion and edits),
3. execute funscripts (drive a playback device),
4. generate images,
5. play video or audio.

The owner's stated safety model is an **allowlist on tools**, on the basis that a
closed tool set cannot be made to do anything outside it. This ADR accepts that
direction and records what an allowlist does and does not buy, because the residual
risk of this feature is not code injection. It is a well-formed call that a human
would not have authorised.

## Decision

The assistant becomes a conversational front end over a fixed, **deny-by-default
tool broker**. Every tool call is a typed, schema-versioned request that the Engine
validates before any effect. Conversational text is never parsed as a command, and
no tool accepts a filesystem path, URL, credential, model declaration or executable
job from model output.

Tools are partitioned into three capability classes with different approval rules.
Existing one-use-token doctrine applies: approvals are single-use, digest-bound and
expire; they are not standing permissions.

| Class | Meaning | Approval |
| --- | --- | --- |
| **R — read** | Reads project, catalogue or review state. Produces text, no mutation. | None. Bounded by grant Read and per-turn budget. |
| **A — author** | Produces a *candidate*: proposal, intent or edit recipe. Never mutates the committed master. | None to propose; the **existing separate approval** (generation approval, edit approval, creative approval) is still required before any effect, plus ordinary candidate review and commit. |
| **E — effect** | Has a physical, presentational or irreversible consequence: device playback, media presentation, image generation. | **Fresh single-use human approval per call**, rendering a plain-language summary and the exact digest of what will happen. |
| **S — stop** | Retires motion or presentation. | **Never gated.** See below. |

**Stop is unconditional.** `device.stop` and `media.pause` must be executable by
the assistant without approval, at any time, including while an approval for the
opposing action is pending. A stop that requires consent is not a stop. This is an
inversion of the default-deny rule and is intentional.

Approval for an E-class call is bound to: the exact tool, its canonical argument
digest, the project and revision, the acting session, the originating attempt and a
single-use claim. Changed arguments, a changed revision, a withdrawn profile or a
restarted Engine invalidate the approval. A late approval cannot authorise a
substituted action.

## Why an allowlist is necessary but not sufficient

An allowlist removes the ability to reach unintended capability. It does not
address four residual risks, which this design must therefore handle explicitly.

1. **Untrusted content steering a legitimate tool.** Media metadata, embedded
   subtitle text, imported `.funscript` comments, project names and Stash-provided
   fields are all attacker-influenced strings that reach the model through context.
   The mitigation is not a better allowlist but the class split: content can
   influence the *authorship* of a candidate, and can never satisfy an E-class
   approval. A injected instruction can at most produce a proposal a human must
   review, or an effect a human must approve with its consequence shown.
2. **Well-formed but harmful sequences.** Playback is physically and
   psychologically consequential even when every call validates. Approval must
   present the concrete consequence (which script, which device, expected duration,
   travel range and declared dynamics limits), not the tool name.
3. **Budget and resource exhaustion.** A conversing model can loop. Per-turn tool
   budgets, per-session attempt caps and refusal of rather than truncation of
   oversized requests are required, following the existing bounded-retention rules.
4. **The transcript is the sensitive payload.** SEC-03 requires exported logs to
   avoid prompts and content. For roleplay the conversation *is* the content, so
   transcripts are local project-adjacent state with their own explicit retention
   and deletion, excluded from evidence receipts, diagnostics and support bundles by
   construction.

## Tool allowlist (exhaustive)

Anything absent from this table is refused. The Engine rejects unknown tool names,
unknown fields (`deny_unknown_fields`) and any request that exceeds its declared
budget.

| Tool | Class | Existing Engine surface | Notes |
| --- | --- | --- | --- |
| `project.summary` | R | authenticated project/revision reads | Current project, revision, tracks, protection summary. |
| `review.explain` | R | assistant review explanations | Explains recorded flag reasons and confidence; cannot invent flags. |
| `catalog.list` | R | `AssistantEditRecipes`, creative patterns | Edit recipes and pattern/preset catalogue for the current revision. |
| `motion.propose_patterns` | A | `StartAssistant` creative scope → `AssistantCreativeApproval` | Existing constrained creative path. |
| `generation.propose` | A | `GenerationIntent` + separate generation approval | Captures settings and effective stride; saving starts nothing. |
| `edit.propose_recipe` | A | `AssistantEditRecipes` → `ApplyAssistantEdit` | Existing bounded recipe ops; protected segments preserved. |
| `target.select` | A | assistant target proposal | Derives the checked seed and captured view for a later generation. |
| `commit.propose` | A | ordinary candidate review/commit | Proposes; the human commit action remains separate and unchanged. |
| `script.play` | E | `PrepareHandyPlayback` → `StartHandyPlayback` / `PlayHandyHspLive` | Requires existing timing admission, controller lease and stop guard. |
| `device.stop` | S | `StopHandyHspLive`, `CancelHandyPlayback` | Ungated. |
| `device.status` | R | `HandyPlaybackStatus`, `HandySessions` | Observation only; not a liveness claim. |
| `media.present` | E | desktop/browser player surface | Play/pause/seek an **app-owned** player over a local file. |
| `media.pause` | S | player surface | Ungated. |
| `image.generate` | E | **none — new endpoint required** | See below. |

Always refused regardless of approval: shell or process execution, filesystem
paths, network or URL fetches, credential or key access, assistant profile or model
registration, protection unlock, project export/import or package publication, Stash
publication, update installation, and any settings mutation outside a tool's
declared bounded fields.

## What is genuinely new surface

Three items are new capability, not exposure of an existing one. Each needs its own
implementation and qualification.

1. **`image.generate`.** No Engine image-output endpoint exists today.
   `image_generation_origin` records image *input* provenance for image-conditioned
   creative synthesis, which is a different feature. Image output adds a model/pack
   dependency, a redistribution review (SEC-08), a generated-imagery safety policy
   and an output-storage contract. This is the largest new dependency in this ADR.
2. **`media.present`.** The desktop and browser players exist, but their control is
   currently a human UI path. An assistant-controlled presentation needs bounded
   commands plus the rule that presenting media never grants device authority, and
   that a device playing from a script and a player presenting media keep separate
   timing admissions.
3. **Conversation state.** Current attempts are one-shot, bounded to 4 KiB of user
   text, optionally image-bound, fenced by the original request ID and given no
   history. Roleplay is stateful. Conversation state is app-owned, local, bounded,
   per-project, separately retained and never published into a project master.

## Authority boundaries that do not change

- The assistant never becomes a timing authority. `script.play` composes existing
  timing admission, controller lease and player guard; it cannot mint or extend any
  of them.
- SAFE-01/02 stand unchanged: after a source or device disconnect, playback remains
  disarmed until the **user** explicitly resumes. The assistant cannot re-arm on
  reconnect and cannot be the consent for resumption.
- DEV-04 stands unchanged: online transmission of a script or command still requires
  explicit user opt-in for that transmission. An approval to play is not permission
  to disclose source media or prompts, and no tool can move content off the machine.
- MEM-01/MEM-02 stand unchanged: an assistant request may pause and checkpoint
  offline generation, and live device playback remains protected from that handoff.
  A conversing assistant with an active image model makes this contention constant
  rather than occasional.
- EDIT-05 stands unchanged: text and media content are data. The class split is the
  mechanism that keeps that true while allowing effect tools to exist.

## Budgets (design defaults)

Proposal: at most 4 tool calls per turn and 32 per session by default; 8 KiB per
tool-call encoding; A-class candidates bounded to the existing catalogue limits;
at most one E-class approval pending at a time; conversation state bounded per
project with explicit deletion; refusal rather than truncation on overflow. Exact
numbers are design defaults (D) until measured.

## Open decisions

| ID | Open item | Why it blocks |
| --- | --- | --- |
| RPA-OPEN-01 | Is local image generation a launch modality, a Core capability, or an optional pack? | Determines pack size, redistribution review (SEC-08) and the 16 GB memory budget. |
| RPA-OPEN-02 | Which interactive model profile serves roleplay, and what is its floor? | The pinned 0.6B Q8 assistant fails 1 of 8 authored intents and is far from interactive latency; a larger model collides with OPEN-02 memory caps. |
| RPA-OPEN-03 | Roleplay content policy: moderation scope, AI disclosure, participant consent, age policy. | The specification has no provision; this domain requires one before an effect tool exists. |
| RPA-OPEN-04 | Transcript retention, deletion and export-exclusion defaults. | SEC-03 is written for telemetry, not for conversations that are themselves sensitive. |
| RPA-OPEN-05 | Does an approved `script.play` survive a page refresh or Engine restart? | Invocation of any prior receipt as current authority contradicts existing doctrine; the default here is no. |

## Consequences

The allowlist is necessary and is accepted as the primary structural control. It is
not sufficient on its own: the class split, unconditional stop, per-call approval
with consequence disclosure, and transcript handling carry the rest. Requirements
are recorded as RPA/TOOL items in the specification amendment, and three acceptance
gates (M3-ROLEPLAY, M4-IMAGEGEN, M5-CONDUCT) are added to the execution ledger. The
25 original gates are unchanged and no existing acceptance criterion is relaxed.
