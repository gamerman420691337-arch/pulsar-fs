# ADR B5-043 — Independent stop custody and guarded dispatch claims

Keep the expiry monitor independent of database and network work, and give the Engine a
separate stop/completion worker that retains shared runtime, transport and journal owners
without retaining Engine itself. Capture a current stop intent under control ownership,
record it after releasing that lock, then recheck the exact effect before dispatch. This
keeps a blocked journal writer from preventing controller retirement and avoids an Engine
lifetime cycle or a worker attempting to join itself. Repeated retirement cannot mint a
new stop after an attempted stop; an acknowledgment still leaves physical stopping unknown.

Require the dispatch authorizer to invoke a sealed, invocation-bound claim continuation
while its dependency-mutation guard is still held. Returning checked facts before claiming
would allow a commit/retirement race. Release all authority/control guards before HTTP.
An error after consuming the claim remains Unknown. Native play authorization and its
public request/recovery lifecycle still need integration; this decision does not enable play.
