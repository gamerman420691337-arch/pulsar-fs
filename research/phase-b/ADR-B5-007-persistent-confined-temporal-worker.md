# ADR B5-007: persistent confined temporal worker

Date: 2026-10-04
Status: Implemented worker primitive; authenticated session integration and qualification open

Finite job execution cannot provide efficient continuous analysis by reloading a model for every frame pair. Add a distinct private worker role which loads one fixed-profile CPU temporal estimator, then handles bounded ordered pair input. Reuse the finite launcher's confinement builder rather than creating a second isolation policy.

Bind readiness and each result to attempt, capture/source/project/revision, complete profile digest, declared clock and exact issued pixel hashes. Accept opaque pixel-runtime work in the parent; borrow it so the capture owner retains scheduler/freshness authority. Results remain unqualified inferred positions and never commit motion or authorize dispatch.

Reserve aggregate worker admission and snapshot storage. Execute/load exact captured worker/model/runtime bytes through readonly guest bindings. Apply an independent cancellation/expiry watchdog and mandatory owned-process reap on drop/failure. Bound both stream channels and body sizes, and refuse provider substitution, mismatched identity and expired sessions.

The native authored fixture establishes multiple actual ONNX runs in one confined process, cancellation, idle-watchdog reap, exact dependency refusal and unsupported-profile refusal. It does not establish authenticated live integration, real acquisition, calibrated clocks, model quality, GPU six-axis capability or physical response. Those remain required successors. Capture limits stay honest until that integration exists.

Evidence: [worker checkpoint](../../docs/evidence/roadmap-2026-10-01/persistent-live-worker/validation.json). Model weights/training and the prior negative sterile evaluation are unchanged. No schema/portable/Stash proof version change is needed for this private worker primitive.
