# Handy command recovery and explicit resumption

This recovery path resolves the custody of exact original device commands while retaining their
original outcomes. An `Unknown` setup remains `Unknown`; a separate resolution records what was
later established. Recovery does not start playback, establish loaded script contents, or restore
controller and timing authority.

The software implementation and declared local checks pass. The full default Engine suite passes
1,172 tests with 70 existing ignores; Protocol passes 353 and Handy clients pass 52 with one existing
desktop ignore. Both strict lint configurations, builds, architecture and format checks pass.
No real Handy was contacted or physically qualified.

## Required evidence

Recovery requires a current accepted physical qualification subject at version 5. Its explicit
contract declares when earlier commands settle after host dispatch drains, which remote stopped
state is meaningful, the required sample separation, and the maximum observation span. The
separation must cover the qualified physical stopping bound. These are evidence requirements,
not vendor guarantees or supplied default delays. Versions 1–4 remain readable but cannot
authorize this recovery path.

The Engine checks the original physical subjects against the accepted device identity and
operative settings. A matching credential route alone is insufficient. Changed physical subjects,
unknown legacy routing and changed route namespaces remain unresolved by this path.

## CLI workflow

Use an existing project, current connection and accepted qualification. Owner proof is loaded by
the existing client mechanism; credential values are not command arguments.

```text
pulsar handy-reconcile-plan PROJECT --device-id DEVICE --registration-revision REVISION --engine-epoch EPOCH --device-session SESSION --qualification-id DOSSIER --qualification-revision ACCEPTED_REVISION
pulsar handy-reconcile PROJECT --plan-id PLAN --request-id ORIGINAL --allow-online-inspection --allow-resolution
pulsar handy-reconcile-status PROJECT --reconciliation-request-id ORIGINAL
pulsar handy-reconcile-cancel PROJECT --reconciliation-request-id ORIGINAL --request-id CANCELLATION
```

Planning performs no device I/O. It captures exact outstanding history and requires scope over
every affected project. Plans are bounded and expire; a restart requires a new plan. Starting
recovery reserves the original request and launches owned work. Poll its original request until
it completes, fails, is cancelled or is interrupted. Exact retries inspect history and cannot
repeat sampling or restart a missing worker. Cancellation has its own request identity; its
receipt and the original cancelled state commit together. A committed resolution wins a later
cancellation race.

The worker retires preparations for the selected device and route aliases, drains their commands,
persists attempted outcomes, then collects fresh matching observations under the accepted contract.
Locally rejected effects stay retained until the durable resolution covers them. Metadata I/O
does not hold the preparation registry, and unrelated execution retains its own stop owner.
Renewal and timing inspection remain available to unrelated controllers. Drain/finish waits use
a lightweight command handle and do not hold the execution-owner mutex. Selected retired
controllers and removed synchronization contexts cannot be restored by those callbacks.

After resolution, use new explicit setup, preparation, controller, clock and play operations. Old
setup records, claims, preparations and receipts cannot become executable again. See the
[play submission](PULSAR_HANDY_PLAY_SUBMISSION.md), [playback clock](PULSAR_HANDY_PLAYBACK_CLOCK.md)
and [stop status](PULSAR_HANDY_EMERGENCY_STOP.md) guides. `playback_enabled` and
`authority_restored` remain false in recovery responses.

## Evidence boundary

Journal version 8 preserves original row bytes and terminal outcomes, validates exact resolution
membership, and maintains separate unresolved setup and synchronization route caches. Older
readers must refuse it. Startup verifies completed main-database witnesses against the independent
journal; response repair cannot recreate native proof.

The native CLI campaign verifies status/cancellation/restart, three metadata-only repairs, actual
v7 refusal of v8, and six corrupt or missing record cases. Existing database and WAL bytes are
preserved on refusal; creation of an absent empty WAL is explicitly recorded. Its 41 owned
processes joined without forced kills. The separate native device-journal regression also passes.
Initial failed test-fixture and harness attempts remain retained alongside their diagnoses.

The retained campaign is under
`/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-22/handy-reconciliation`.
Injected observations, synthetic historical fixtures and localhost transport tests establish only
their software scope. Browser synchronization, continuous timing renewal, physical behavior,
real-media/model quality and release acceptance remain separate open roadmap obligations.


## HSP extension

The same commands now support the explicitly accepted HSP transport through
[subject v7 and journal v11](PULSAR_HANDY_HSP_RECOVERY.md). Mode-4 stopped samples,
original qualification links and exact HSP stream/effect/emergency membership
are required. Legacy hosted acceptance cannot authorize this branch; missing
historical links are not inferred. A native admission fence persists through
failed or cancelled recovery. Committed resolution releases exact drained owners,
while old handles remain retired and future playback needs fresh preparation.
