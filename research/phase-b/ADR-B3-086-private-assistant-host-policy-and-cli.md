# ADR B3-086: private assistant host policy and exact first-party intents

Status: actual Linux host-policy/CLI/IPC proposal flow verified; effects/desktop/bundling and broad regressions incomplete.

## Decision

Load an explicitly approved, private bounded host policy before listener publication. Keep paths out of client requests; pin the captured worker and declared native assets, limits and aggregate feasibility. Registration is scoped to one Engine lifetime, with no automatic reload/replacement/default selection. Use safe existing rustix APIs for private client intent publication. Persist exact endpoint/session/credential digest, original request ID, profile, revision and text before submission; require explicit inference approval and refuse cross-namespace retry. Reconciliation resubmits original facts and never invents a new profile/identity.

## Evidence and limits

Five focused policy/intent tests and one real traced executable CLI/IPC/Qwen3 proposal flow pass. Initial unsafe/trait compilation and untraced 50-second readiness failure are retained. Traced sync latency reaches 18.64/13.07 seconds without proving the earlier failure's cause. Full current engine/client suites remain FAIL with test-helper/recovery timing failures; no deadlines/sync rules are weakened. Actual effects, first-party desktop, offline packs, memory handoff and wider qualification remain required. See [setup, commands and exact evidence](../../docs/PULSAR_ASSISTANT_HOST_AND_CLI.md).
