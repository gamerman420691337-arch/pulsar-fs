# ADR B5-044 — Original play operations and durable claims

Record a play operation and its exact effect together in device-journal version 5, binding
the original actor/request fingerprint, acknowledged setup and derived preparation, controller,
qualification, clock snapshot and encoded effect. Release the effect receipt only after a
durable single-use claim, and commit operation/effect outcomes together. This preserves an
explicit unclaimed cancellation boundary without treating legacy prepared effects as unsent;
legacy rows may already have been transmitted and cannot be adopted by the new operation type.

Keep history separate from current authority. A restart fences claims and completions, exact
repeats never recreate a native queue, and cancellation changes metadata without clearing runtime
or physical uncertainty. The Engine still needs original-request reservation/witness integration,
fresh authorization through the claim continuation and shared play/stop ownership before public
play can be exposed. Older journal readers refuse version 5; no historical rows are rewritten.
