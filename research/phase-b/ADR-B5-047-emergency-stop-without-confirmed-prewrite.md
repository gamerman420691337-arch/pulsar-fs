# ADR B5-047 — Emergency stop without a confirmed prewrite

Permit a current sealed native stop to use the existing dispatcher when persistence cannot
confirm its ordinary prewrite. Keep this envelope separate from play and durable receipts.
Both stop paths consume the same runtime claim sequence, check the captured session/configuration
and transport, and preserve physical uncertainty. A known foreign journal owner still refuses.

Retain actual attempted outcomes independently of the journal, then retry only an atomic metadata
write. Version 7 records emergency provenance alongside the unchanged generic stop effect and
terminal outcome. A bounded census detects missing markers. An already-prepared identical effect
is allowed because a failed prewrite may have committed. Rejected handoffs do not become fabricated
transport observations. Shutdown reports unresolved stop dispatch/metadata after joining.

Expose bounded stop history and current-process activity through a separate read-only status
command tied to an original controller acquisition. Always disclose incomplete history; restart
cannot restore lost volatile observations or native authority. This does not qualify physical
emergency behavior or resolve attempted-effect custody, browser timing or user resumption.
