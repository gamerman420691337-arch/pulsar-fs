# Durable original browser gap commits

Gap commit preparation now persists the exact explicitly acknowledged candidate,
base revision, program digest and original request ID before dispatch. The optional
journal field preserves older upload-only records without inventing approval.
Recovered uploads still restore no review pin. An approved original commit may be
explicitly retried; creating any new commit still requires fresh shared diagnostics
and acknowledgement. Startup and inspection never execute a commit.

The replay path holds the private workflow lock, checks session/credential/project
bindings and the original candidate/pin against authenticated Engine readback,
then uses the original `CommitCandidate` request and expected base revision.
The Engine's durable acknowledgement distinguishes original acknowledged revision
from the current project revision. Retaining an acknowledgement is checkpointed;
unknown outcomes preserve the original request and cannot create replacement work.
The journal is an inert record, not a grant; every replay still requires current
Engine authentication/authority and existing stale-revision/idempotency rules.

After relaunching browser-live for the same project/session, press **Inspect or
recover gap proposal intent**. If it lists an approved original commit request,
press **Retry original commit request** to reconcile it. Do not prepare a new
commit to resolve a lost acknowledgement. Upload-only records require fresh review
and explicit acknowledgement before commitment.

This scope covers commits of durable gap proposals. Original recording-candidate recovery, managed retirement/release, broader filesystem/crash
coverage and platform qualification remain unfinished. Mixed older binaries may
refuse the new optional commit field; preserve their journal rather than replacing
it. No motion quality, physical device or human release acceptance is created.

## Verification scope

The network-disabled native campaign retains a prepared original commit before
Engine dispatch, rejects a deliberately altered program pin without advancing
revision one, then drops the real successful gap-commit reply. SQL confirms that
revision two already exists. After an abrupt server kill and restart, inspection
retains the original request with unconfirmed acknowledgement, a new preparation
without fresh review refuses, and explicit original retry returns revision two.
Three commit calls share one request ID and one dropped reply; retries create no
new revision. The campaign also retains original upload recovery through two
earlier server kills. All 493 inferred points remain; five synthesized repairs
produce 498 neutral actions. Two candidates, two exports and no ordinary jobs
remain; final custody is empty and owned processes/proxy join.

Rendered in-app browser QA records 529 estimates, proposes five repairs and
explicitly approves their commit. Its proxy drops the successful reply, and the
page reports the unconfirmed outcome. After a graceful server restart, inspection
shows the same approved original request, explicit retry acknowledges revision two
and repeated retry keeps it. The actual downloaded 534-action script is strictly
ordered, every position is 38, and endpoints are 0 and 60000 ms. Proxy receipts,
pre/post-restart accessibility states, screenshot and exact bytes are retained.
This is an authored scalar diagnostic fixture, not useful learned motion.

Clients pass 421 tests with ten ignored, thirteen mock-player cases and ten strict
validation commands pass, including both Clippy configurations, native builds,
Windows source compilation, formatting, architecture and guards. Legacy upload-only
records, missing approval, changed candidate pins and invalid acknowledgement
revisions have scoped controls. Core/Protocol/Engine sources are unchanged; no new
full suites are claimed for them. All 549 source identities, two native binaries
and sixteen borrowed runtime files are reverified.

No Engine-restart-after-lost-gap-commit or mid-journal-rename crash campaign is
claimed. Recording/export recovery, managed release, broad platform/concurrency,
useful model/media quality, arbitrary-tab capture, GPU live six-axis, physical
Handy sync and human/release acceptance remain open. Full roadmap PARTIAL.

See [checkpoint](evidence/roadmap-2026-10-13/browser-gap-commit-recovery/checkpoint.json)
and [ADR B3-198](../research/phase-b/ADR-B3-198-browser-gap-commit-recovery.md).

Subsequent [script export recovery](PULSAR_BROWSER_EXPORT_RECOVERY.md) adds original export inspection/retry after server loss; managed release remains open.
