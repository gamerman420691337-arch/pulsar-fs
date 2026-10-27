# Inspect preserved retiming facts in a cloned project

The authenticated engine now provides a separate read-only lookup for imported historical retiming:

```text
pulsar project archived-retime-status PROJECT --revision RETIME_REVISION
```

The desktop Source placements panel exposes the same lookup under Retiming receipts, alongside the existing local receipt query. Select the historical edit revision, then choose Imported receipt. The result distinguishes historical attribution from a locally recorded edit. Changing the project or selected receipt revision discards stale presentation; the panel does not authorize or calculate the transformation.

The command requires current Read authority on the destination project. It requires no expected current revision and changes no project motion, history, grant, request intent or publication state. An unimported revision, an absent legacy receipt or a revision beyond the imported capture returns NotFound. `project retime-status` remains the separate local-authentication contract and still returns NotFound for imported historical edits.

## Verification and correspondence

Before allocating SQLite text, the reader bounds the retained clone manifest, identity-map and package descriptors, local placement/program descriptors, attribution and membership cells. Explicit memory reservations cover decoded metadata, each retained original manifest and the selected historical motion transformation. Resource refusal returns no partially verified receipt.

The reader validates the canonical retained clone projection and independently reads the hash/length-verified identity-map object through the engine's pinned evidence directory. Its complete candidate/placement map must match the immediate import namespace. It then reads every required canonical original manifest through the same descriptor-relative, no-symlink path and validates the bounded, exact reachable origin graph, namespace mappings and their composition. Import aggregate accounting now includes placement closure, retiming receipts and placement mappings; the pure graph check also charges the root logical records.

Exactly one preserved native receipt must correspond to the requested local revision. Original receipt facts stay unchanged. The reader maps its base/result revisions, old/new placements and actor into the destination namespace, checks the recorded local actor/edit kind/parent, complete retained placement definitions and target membership at that historical revision, and binds both local motion descriptors to the original exact identities. It reads the selected hash-verified programs and recomputes the full declared retime, requiring complete result equality.

The response contains the destination correspondence, original manifest identity and unchanged original receipt, with `historical_attribution_only: true`. Original sessions, requests and actors are not registered, replayed or granted destination authority. The client checks the strict response and destination project identity before presenting it. The typed response refuses a claim of local attribution or a fabricated qualification field.

This status checks selected motion and all required manifest/mapping bytes. It does not reconstruct every ancestor's complete flat container or re-read all original source-media payloads; that complete-container reconstruction belongs to import validation. It does not authenticate an external person, establish cryptographic nonrepudiation, resolve media alignment, qualify perception or admit device playback.

## Evidence and remaining work

Protocol tests cover historical-only attribution, distinct clone identity, fresh placements, strict decoding and scoped nonmutating command behavior. The compiled CLI fixture exercises the lookup after two successive clones, Read-only grants and revocation, corrupted original-manifest refusal, no-symlink refusal, oversized SQLite manifest refusal, changed local attribution refusal, restoration and restart with unchanged motion. Existing local status remains separate. Desktop controls compile; manual GUI usability remains unqualified.

Database schema remains 6, portable project format remains 5 and private Stash timing proof schemas remain 1/2. Executable clock-proof composition across clone namespaces and portable Stash capture/source/proof association migration remain implementation work. No actual Stash or Handy instance is contacted. Exact current source/binary identities and raw gates are retained in [this increment's receipt](evidence/roadmap-2026-10-01/archival-retiming-status/validation.json).
