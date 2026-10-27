# Explicit Handy temporary script hosting

Pulsar can submit one exactly prepared Stroke CSV to Handy's temporary public hosting
service. This discloses the motion samples outside the computer. It requires an explicit
hosting command, consent for that exact script hash/revision, the host-local Handy owner
capability, and both Read and Export grants. It never uploads media, prompts, provenance,
API keys or connection keys, and it does not set up or actuate a device.

These controls are CLI-only. The development executable is
`/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar`; substitute that path for
`pulsar` below if it is not installed on PATH.

## Workflow

First use [local preparation](PULSAR_HANDY_SCRIPT_PREPARATION.md), retaining its original
request ID and receipt. Review the file and receipt. `handy-sessions` returns the current
Engine epoch without contacting a device. Use the same authenticated client session that
prepared the script:

```sh
pulsar handy-host-stroke PROJECT --revision REVISION \
  --preparation-request-id PREPARATION_ID --script-sha256 SHA256 \
  --engine-epoch ENGINE_EPOCH --request-id HOSTING_ID \
  --allow-temporary-public-hosting

pulsar handy-hosting-status PROJECT HOSTING_ID

pulsar handy-forget-hosting PROJECT HOSTING_ID --request-id FORGET_ID
```

Hosting and forgetting each need their own original request ID. The Engine resolves the
completed preparation receipt, checks the current project/master binding, opens the saved
file without following a final symlink, and reads bounded bytes. Changed bytes refuse before
an upload reservation. Only the captured bytes whose hash matches the receipt enter the
multipart request; subsequent path changes cannot retarget that disclosure.

`unknown` means the reserved disclosure may have occurred without a verified result.
It is not a request to upload again. Exact replay inspects the original request and never
reopens the file or repeats the POST. A changed original intent conflicts. If a later
upload is deliberately desired, it needs a fresh explicit intent and ID; no automatic
fallback or retry creates one.

`verified` means the returned download's numeric CSV rows matched the authorized samples.
It is historical content verification, not continuous URL availability, script review,
physical qualification, loaded-device state or playback permission. `link_available` means
the current Engine retains the native location and its project binding is current.
`upload_in_flight` reports local work that has not retired. These live fields are not persisted.

Forgetting retires the local location. It does not delete the server's copy or cancel an
already reserved disclosure. If network work is still running, status keeps reporting it
until that bounded call returns. A late callback cannot restore a forgotten location.
Remote retention is controlled by the provider; no deletion or expiry duration is promised.

## Recovery and authority

The Engine commits an unknown-outcome receipt before network I/O. A verified download
can update that same receipt, preserving its captured identity. If completion persistence
fails, the original uncertainty remains and no duplicate upload follows. Replays and status
require current owner authentication and project grants. A changed revision or revoked grant
during I/O cannot install a usable location, even when the captured download was verified.

Restart restores historical observations but no URLs, in-flight work or consent to send
again. An original request already reserved may return history; an unclaimed old-epoch
request refuses. Native locations are omitted from Pulsar receipts, errors and the request
journal. Embedding hosts must keep dependency request tracing disabled when locations must
remain out of diagnostics; the HTTP dependency's trace mode can expose request paths.

At most 64 native hosting entries are retained. Forgetting a completed entry releases its
slot; an in-flight forgotten entry stays reserved until its call retires. No background
polling, refresh, re-upload or device command is created by opening status.

## Transport contract and limits

The broker uses the fixed manufacturer HTTPS Hosting v2 upload endpoint, no proxy,
no redirects and no device credentials. It sends one multipart file named `script.csv`.
Both POST and verification GET share one absolute 30-second operational budget; this is
not a qualified network or device timing bound. Upload input is capped at 4 MiB, response
metadata at 64 KiB, and downloaded CSV at 8 MiB. CSV comparison retains row order, integer
milliseconds and integer percentages; comments, line endings, whitespace and leading zeros
may differ. Missing/extra/reordered points, changed values, malformed rows or overflow refuse
verification. An HTTP success alone cannot produce a verified observation.

Returned URLs must use the exact manufacturer origin and one of the plain download path
forms documented by the [Hosting v2 schema](https://www.handyfeeling.com/api/hosting/v2/docs/spec.yaml).
Identifiers are bounded ASCII letters/digits/underscore/hyphen; query strings, fragments,
user information, alternate hosts, traversal and token/partner URLs refuse. The owning
schema was re-fetched and hash-pinned in the evidence packet. This narrow policy deliberately
does not guess at undocumented redirects or service changes.

Live vendor TLS/DNS behavior, service conversion/retention and provider integration still
need real qualification. HSSP setup, loaded-stream and credential-session binding, fresh
clock/identity checks, controller teardown and actual Handy stopping/synchronization remain
unfinished. Public physical playback stays disabled. See
[decision](../research/phase-b/ADR-B5-027-explicit-handy-hosting.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-hosting/checkpoint.json).


## Scoped verification

The complete Protocol suite passes 311 tests, Engine Handy tests pass 63, and Client
Handy tests pass 17 with one existing isolated desktop case skipped. Local HTTP fixtures
verify the multipart payload, absence of device credentials, exact row correspondence,
redirect/error/untrusted-location refusal, and one deadline shared across upload and
verification. Engine cases exercise durable-before-disclosure reservation, exact replay,
changed input/authority refusal, forgetting during I/O, late revocation, completion-write
failure, panic cleanup and restart without restored locations or delayed consent.

Strict default/all-feature workspace lint, App/Worker builds, format and architecture
checks pass with incremental compilation disabled for that validation run. The first
ordinary incremental test link failed on anonymous LLVM symbols; the same source passed
without incremental compilation. A normal incremental recheck reproduced the link error.
Both failed objects were located in one Protocol incremental directory. After auditing
references, that one directory was quarantined; a fresh ordinary incremental rebuild
passed all 187 Protocol library tests using the unchanged linker. Other Protocol cache
variants and source assertions were preserved. This isolates a generated-cache interaction;
it does not establish a compiler/linker implementation root cause.

The native CLI campaign has only a loopback interface. It verifies consent, changed-byte
refusal before reservation, a real offline transport failure retained as unknown, exact
replay without reopening a changed file, conflicting-intent refusal, forgetting, restart
history and rejection of unclaimed old-epoch consent. Its two Engine processes are joined
and worker custody is empty. No script data reaches the vendor and no device is contacted.
Successful hosting is covered by local HTTP and injected Engine observations, not a live
vendor qualification. Initial test-path/deadline fixture setup corrections and a native
error-wording mismatch are retained; the latter now asserts the typed RequestConflict.

Full ordinary-storage suites, rendered desktop hosting controls, live provider behavior,
HSSP setup, hardware timing/stopping, models/media, platform and human/release acceptance
remain open. These scoped checks do not close the roadmap.
