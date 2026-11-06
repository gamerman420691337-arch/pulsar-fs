# Assistant selection of recorded review evidence

The local assistant can propose a recorded master-review flag, and the first-party CLI and desktop can display its exact Engine record. The output preserves reason code, interval, axis, evidence kind, optional uncalibrated confidence and current review decisions. Missing confidence stays missing. This increment adds evidence selection and display; no model-authored narrative or inferred confidence is stored as evidence.

## Context and focus

Assistant Start captures current revision lineage under its existing transaction. Before deserializing review state, it checks the stored cell is at most 512 KiB and reserves bounded metadata memory. This is an assistant capture guard, not a global repair of all corrupt-cell readers. Unique flags are indexed by the SHA-256 identity of their complete validated record. The default context offers the first eight sorted identities. Optional `review_focus` accepts up to eight unique recorded SHA-256 IDs and can select flags outside that default subset. Missing IDs refuse the request. The complete context remains within the existing 8192-byte worker bound; oversized contexts refuse rather than truncate facts. Empty focus is omitted from serialized requests, preserving earlier private intents and request fingerprints.

Where review flags are offered, context carries the actual flags and unknown/conservative lineage status. No-flags context preserves the earlier generation context. Recorded reasons are data. The model may select an offered `explain_review` ID or another offered proposal; it cannot introduce evidence. No candidate flags, targets or edit recipes are offered by this increment. Approved stride generation reconstructs these same selected review facts when checking context freshness, including changed lineage status.

## First-party use

Use `pulsar review-status PROJECT` to discover current recorded IDs. Prepare an assistant request with the existing profile, prompt, revision, request ID and private output options; append repeated `--review-id ID` options to focus the request. Start still requires the separate explicit inference approval. After a completed ExplainReview proposal, run `pulsar assistant explain PROJECT ATTEMPT --revision REVISION`. This performs authenticated status and ReviewStatus reads; it does not run inference or change decisions, motion or device state.

The desktop accepts space-separated review IDs and displays them in its saved approval summary. A completed ExplainReview proposal has a Show recorded review reason and confidence button. Reads verify session, project, current revision, completed proposal, offered ID and the identity of the returned flag. Stale, missing or changed evidence is refused. Cached explanations clear on view/attempt changes and new relevant requests; late responses from another attempt cannot replace the selected explanation. Read-only responses preserve pending mutation retry identity.

## Qualification

Source-bound checks and their actual outcomes are recorded in [the checkpoint](evidence/roadmap-2026-10-10/assistant-review-explanations/checkpoint.json). Regression fixtures seed explicitly authored review records in private test databases and use retained synthetic assistant reports. These do not establish learned model selection, real-media review accuracy, native desktop rendering, model quality, ordinary filesystem durability or user acceptance. The full roadmap remains incomplete. Target selection, protected undoable assistant edits, checkpoint/live playback/resume, installed offline defaults, annotation/evaluation, packaging/platform and Handy qualification remain independent work.
