# WASM start-resolution model and correspondence

The finite model has two actors, one original request namespace per actor, two abstract captured metadata references, independent current Read/Edit grants, delayed preflight/publication, one identified resolution and explicit replay. It models the reservation and acknowledgement as one transition. It does not prove SQLite atomicity, digest collision resistance, descriptor capture, storage failure, more request IDs, physical quota, eventual completion, or general implementation correctness.

A withdrawn original ID must remain withdrawn. Accepted and withdrawn resolution outcomes must correspond to the same namespace and captured reference. Resolution and replay require current Read and Edit authority, including after revocation. The positive model has 6,400 reachable states with the supplied configuration. Four separate source mutations must fail the specific declared invariant with TLC counterexample exit 12; syntax errors, arbitrary failures and timeouts cannot satisfy this adequacy gate.

Run with the exact Phase A TLC jar (SHA-256 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88), an existing scratch directory and a fresh evidence output directory:

```sh
python3 assurance/check-wasm-start-resolution.py --jar /path/to/tla2tools.jar --scratch-root /mnt/4tb/tmp/pulsar-formal --output /mnt/4tb/tmp/pulsar-formal/results
```

The gate records raw positive/mutant output, pinned tool and source/config identities, return codes and the expected invariant for each counterexample. It removes temporary TLC state directories even on a refused or timed-out case. Its output is evidence; the caller retains or removes it deliberately. Tool execution requires Java and is a development gate, not a shipped product dependency.

| Model element | Source and independent concrete correspondence |
| --- | --- |
| PublishStart after preflight | `wasm_edit_execution.rs` rechecks the global request identity in the insertion transaction. The ignored native resolution fixture pauses a real start after preflight, resolves withdrawal and then observes RequestConflict with no invocation or worker. |
| Atomic withdrawal and resolution | `wasm_start_resolution.rs` reserves the original ID and remembers the outer resolution inside one immediate transaction. The ordinary rollback fixture injects failure before acknowledgement insertion and verifies that the original reservation is absent. |
| Accepted reference correspondence | Resolution compares all captured metadata against the retained invocation. The native fixture executes a real guest, recovers the exact attempt/receipt and refuses changed metadata. |
| Read/Edit authority for resolution and replay | Authorization precedes replay inside the transaction. The native fixture separately refuses Read-only, Edit-only and revoked replay, then explicitly restores both scopes. |
| Replay and historical outcomes | Matching identified resolution returns recorded outcomes. The actual controller fixture deliberately omits local publication, reconnects and explicitly replays before reclaiming exact unknown local facts. Accepted reports are historical and need separate status for later progress. |
| Existing-reader namespace | The actual authenticated controller fixture reopens the same private state with a separately captured preceding executable and checks delayed original-ID refusal. The finite model does not prove binary compatibility. |

The [start-resolution checkpoint](../docs/evidence/roadmap-2026-10-10/wasm-start-resolution/checkpoint.json) retains the concrete tests; the workspace-gates checkpoint retains this model execution. Those bounded scenarios do not establish full crash/power-loss, GUI, platform, media/model, device or release qualification.
