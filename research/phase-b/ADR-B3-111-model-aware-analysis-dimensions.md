# ADR B3-111: model-aware analysis dimensions

Status: implemented request authoring; quality and performance qualification remain separate.

The clients previously capped generation at 320 by 180 even when a declared detector consumed a larger tensor. For a portrait source and a 640 by 640 model this reduced the decoded frame to 135 by 180 before the detector expanded it. The letterbox adapter cannot recover discarded detail.

Choose the default decoded analysis limit from a declared detector's input width and height when authoring a new request. Preserve the 320 by 180 default for no-model image flow and the fixed 64 by 64 contract for temporal script-position models. Explicit CLI dimensions remain authoritative, including partial overrides; invalid values reach checked protocol validation rather than being silently repaired. No wire schema or archived settings change: persisted generation and Stash batch requests retain their concrete dimensions and configuration identities.

Use one presentation helper in ordinary CLI, Stash CLI, and both desktop generation paths. Stash CLI must resolve a selected pack before choosing omitted dimensions. Bare custom Stash models use the same 640 by 640 tensor default as ordinary CLI; suppliers with another tensor shape must declare the profile or explicit dimensions. The desktop exposes an optional decoded analysis size and displays the effective limit. Temporal mode disables that override and keeps its fixed contract. Preview stays independent and bounded.

The worker remains responsible for fitting flat source/selected-eye aspect ratios without upscaling, viewport projection, frame budgets, profile/model admission and provenance. The declared model tensor is not a claim about original source resolution, target detection quality or required performance. See [behavior and evidence](../../docs/PULSAR_ANALYSIS_DIMENSIONS.md).
