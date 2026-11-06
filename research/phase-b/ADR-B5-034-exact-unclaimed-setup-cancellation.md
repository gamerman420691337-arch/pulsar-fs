# ADR B5-034 — Exact unclaimed setup cancellation with recoverable receipts

Public cancellation selects an original setup request and its exact record identity,
under the original actor's current owner capability and project Read grant. Only an
unclaimed preparation can release its route; claimed, acknowledged and uncertain attempts
require separate reconciliation. Local cancellation provides no evidence of device stopping.

Reserve a distinct cancellation request before changing the setup journal, then settle its
receipt after the journal confirms cancellation. An exact retry can finish this metadata
sequence after a lost reply or receipt-write failure without affecting a replacement setup.
Use a distinct response kind so cancellation receipts cannot be mistaken for original setup
witnesses during cross-store startup validation. Historical settled receipts remain immutable.
