# Stash publication observations

Retain the publication request ID printed by the CLI before dispatch. Desktop now retains an explicit ID before queueing publication and exposes it for observation.

```sh
pulsar stash publication-status PROJECT --publication-request-id PUBLICATION_ID
```

The operation requires the current host-local Stash owner capability and project Export grant. It reads only a dispatch belonging to the same session and project. It makes no Stash request, publishes no file, and changes no dispatch state, project revision, motion or protection. It returns a fresh timestamped observation; a concurrent dispatch-record change asks the caller to repeat the query.

Dispatch state and artifact observation are independent. `complete` records a finished dispatch with its existing successful receipt. `pending` and `failed_or_unknown` remain uncertain even when local source/script bytes and timing proof verify. Those states return no fabricated successful receipt and do not permit scanning, tagging or redispatch.

For source-time exports, the engine verifies the retained proof digest over the same opened file it parses, streaming unused mapping arrays. The header must bind the original request, project/revision, destination, script identity and master. Source/script verification uses the existing descriptor-relative namespace and content checks. Proof and source/script outcomes can differ: a changed proof can coexist with matching source/script bytes from a separately retained successful receipt. A missing legacy source/proof association is `not_recorded`; the engine does not infer it from equal file bytes.

Desktop offers **Observe publication status** and a report containing stored dispatch state, proof observation and source/script observation. It retains the publication request ID when queueing fails or the acknowledgement is uncertain. Interactive usability is not qualified.

Native compiled CLI/authenticated-engine checks cover forward/reverse completed publications, changed script and proof bytes, wrong owner capability, revoked/restored Export authority, successful restart/replay and an explicitly synthetic `failed_or_unknown` state after removing its success response. Matching bytes in that synthetic state remain an observation; the original publication request refuses redispatch and no extra Stash query occurs. This is not a reproduction of a lost acknowledgement or an independently observed remote attachment.

Explicit disposition/adoption of an uncertain publication, durable observation history, portable Stash source/proof association migration, recovery of missing legacy associations and qualification on an installed Stash remain open. Model/media accuracy, physical Handy behavior and release qualification are separate gates.
