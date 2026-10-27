# ADR B5-046 — Owned daemon listeners and explicit Engine retirement

Treat signal-driven shutdown as an explicit ownership transition. A signal-safe atomic flag
closes Engine admission. Both Unix listeners retain their connection tasks and descriptors so
shutdown can interrupt bounded IPC waits and join all accepted work. Socket cleanup checks
identity, and the instance lock remains held through retirement, including partial startup.

Retain detached-style Engine supervisors in a bounded task group with atomic close, outside-lock
joining, panic diagnostics and self-join refusal. Do not join from its destructor: a worker can
own the last Engine reference. Cancel operation registries before joining and retain the existing
native-preparation producer handles. Check native worker custody after supervisor retirement.

Retire Handy authority before waiting for unrelated work. Its final play authorizer observes
shutdown, and its independently owned stop/monitor workers join with original resources alive.
Expose their join failures to daemon shutdown without weakening durable or physical uncertainty.
Client closure continues to permit retained offline Engine work. This does not resolve emergency
stopping under persistence failure, physical stopping proof, abrupt termination, or platform
qualification.
