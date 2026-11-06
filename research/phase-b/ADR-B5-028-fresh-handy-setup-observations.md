# ADR B5-028 — Recheck both native references before device setup

A retained hosting observation and an earlier device inspection are insufficient current
facts for setup. Add one bounded read-only inspection that captures both native references,
brackets an exact hosted-byte check with device observations, and rechecks ownership,
registration, project revision and authorization afterward. Use the same credential
snapshot; do not reload names or accept a client-provided URL.

Require the mode generation locally and compare the observed identity, mode generation and selected HSSP
state fields across the bracket. These checks detect observed changes but cannot establish remote
atomicity, external-controller exclusivity or physical stopping. The remote stream is not
associated with the requested hosted script by inspection alone. Public reports remain
unqualified and explicitly state that setup was not performed.

Keep reference capture inside the owning Engine modules and use narrow native interfaces
for GET observation and exact content recheck. No native URL or credential is serialized,
no report becomes a setup token, and no receipt or permission is restored by replaying a
read-only result. A future mutating setup must freshly acquire qualified authority and
preserve its own durable uncertainty and stop obligations.
