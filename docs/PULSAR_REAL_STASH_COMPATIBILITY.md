# Real Stash authored compatibility checkpoint

Date: 2026-10-09
Status: scoped compatibility PASS; full roadmap PARTIAL; release unqualified

## Scope and identities

The production Pulsar CLI and Engine were exercised against the actual official Stash v0.31.1 binary, not an HTTP mock. The binary reports commit `4de2351e`; its SHA-256 matches the official GitHub release asset digest. The [official release](https://github.com/stashapp/stash/releases/tag/v0.31.1) supplies the pinned version. Exact binary lengths/digests, launch commands, responses and source inventory are retained in [the evidence checkpoint](evidence/roadmap-2026-10-09/real-stash-compatibility/checkpoint.json).

The Stash server used a fresh owned database and configuration inside a Bubblewrap filesystem sandbox. The user home/library were absent from that namespace. Only three authored 64x64, 8 fps, one-second FFV1 videos and one preexisting authored funscript were scanned. The server listened on loopback; authentication was not enabled. No user library, live training references, sterile evaluation references or hardware were exercised. The production supplier is the preceding generation-clock runtime candidate; all 418 source inventory entries remained identical. No Rust or JavaScript implementation changes were needed for this campaign.

## Actual production outcomes

- The real metadata scan finished with three scenes: one interactive scene with an existing sibling script and two noninteractive scenes.
- `stash list-missing` returned exactly the two noninteractive scenes. `stash plan-missing` reported both mapped local sources ready for snapshot.
- A detached engine-owned `generate-missing --scan --tag Pulsar-Authored-Compatibility --create-tag` batch captured, generated, committed and published both selected scenes. Each produced eight actions without a model. Each remains unqualified with one unresolved review item.
- The actual Stash scan jobs finished. Both Pulsar receipts report `verified_linked`, with independent attachment observations reporting `observed_linked` and `remote_script_hash_verified: false`.
- One tag was created and added to both scenes. Independent GraphQL queries observed the exact tag and all three scenes as interactive. Existing-tag preservation was not a case in this campaign.
- A fresh Engine process retained byte-identical finished batch status. Exact submission replay returned the original submission acknowledgement, while a separate status query still returned the unchanged finished ledger. Rediscovery and a new batch selected zero items.
- A supported capture command for a now-interactive scene reached the Engine and refused with `RequestConflict: Stash source is not eligible: AlreadyInteractive`. All published hashes remained unchanged.
- Independent SHA-256/length checks match both new scripts to their publication receipts; all original videos and the preexisting script remained byte-identical. The user's actual Stash configuration and database also remained byte-identical.

## Harness negatives and limits

Three assertions/setup mistakes were retained explicitly: the terminal protocol state is `finished`, not `completed`; submission replay preserves its original acknowledgement rather than synthesizing current status; `capture-source` does not accept `--request-id`. Corrected supported commands reached the production operations and final assertions passed. These are harness negatives, not product defects.

This establishes anonymous loopback compatibility for the sampled official version and authored file mapping. It does not qualify authenticated servers, TLS/proxies, the user's real library, other Stash versions, desktop usability, preexisting tag relationships, remote script bytes, useful model accuracy, sustained performance, other platforms, Handy behavior or human acceptance. Historical mock/adversarial cases remain separate evidence.

## Custody and cleanup

Both owned systemd units ended inactive/dead with MainPID zero and success. The final version-4 worker fence contains no groups, retired groups, staging entries or creation records. Public evidence contains authored media and selected operation records only; private Engine credentials, databases and generated Stash control keys are excluded. The temporary Stash binary and service state are removed after recording terminal/custody checks. No shared services, user data or build caches are removed.
