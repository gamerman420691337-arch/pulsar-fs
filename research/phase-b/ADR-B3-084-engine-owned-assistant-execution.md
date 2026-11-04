# ADR B3-084: Engine-owned assistant execution

Status: trusted one-shot Engine execution implemented and tested on Linux; authenticated application workflow pending. Full roadmap INCOMPLETE.

## Decision

Use the existing shared resource pool, durable worker fence, confinement and retirement custody for learned assistant proposals. Capture declared worker/runtime/model bytes in sealed memfds only after accounting for parent copies plus child RAM. Keep paths owner supplied, guest paths fixed, output strictly bounded and request identity/scope independently checked. Install image ownership before any fallible launch step. Return success only after whole-tree retirement; unresolved cleanup retains images and admission. Preserve the original deadline and exact cancellation flag.

This Rust embedding API grants no session/project authority or effects. Authenticated asynchronous receipts, current-context/approval fencing, protected application actions and generation memory handoff remain next. Read-only system libraries and cooperative host-I/O checks do not establish captured dependency closure or hard filesystem deadlines. No public wire/schema/dependency change.

## Evidence

Four ordinary tests and three explicit Linux custody/transport/cancellation/learned tests pass. Cancellation requires a live owned child before signalling. Actual pinned Qwen3 selects the expected target with complete retirement; quality/default selection remain open. Initial fixture/compile failures and complete regression gates are retained in [implementation and evidence](../../docs/PULSAR_LOCAL_ASSISTANT_PARENT.md).
