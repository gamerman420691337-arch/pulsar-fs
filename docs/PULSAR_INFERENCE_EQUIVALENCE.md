# Inference equivalence records

Pulsar can retain a comparison between two explicit inference profiles and record
the host owner's decision about that comparison. This is a prerequisite for the
roadmap's qualified fallback. Automatic backend substitution and successor job
scheduling remain unavailable.

The first version compares profiles using identical model bytes. Runtime,
provider, input size and preprocessing may differ within the supplied evaluation
protocol. Matching class counts cannot establish that different model files use
the same labels, so different models require a future explicit ontology contract.

## Evidence and scope

An input manifest names the two profiles, their exact software/hardware
environments, quality tier, recipe, axes, source/view policies, limitations and
validity interval. It references eight bounded JSON documents by role, absolute
path, SHA-256 and byte length:

| Role | Required correspondence |
| --- | --- |
| Corpus | Source/reference hashes, case/group/target identities, windows, annotations and evaluation rights |
| Protocol | Supplied metric bounds and allowed differences, plus the protocol approval identity |
| From execution | Explicit original worker requests, input/profile/environment identities, outputs, elapsed time and memory observations |
| To execution | Matching cases and generation settings under the alternate profile |
| Quality comparison | Paired values evaluated against the supplied criteria |
| Provider placement | Provider/run/trace observations bound to the exact executions |
| Resources | Execution-linked wall-time and memory observations; unknown values remain unknown |
| Review | A supplied review conclusion bound to the preceding seven documents |

Import reads bounded regular files, verifies their bytes and checks the typed
cross-document relationships. A new record is always **Pending**. Reimporting
identical evidence preserves its existing decision and revision. Declarations,
hashes and JSON validation do not independently authenticate measurements or
establish scientific quality.

Acceptance requires an explicit host-owner command, a named operator, the exact
current record revision, a currently valid measured evaluation, a frozen heldout
corpus with independent annotations and evaluation rights, complete observations,
passing supplied criteria and a supporting supplied review. Pulsar does not invent
quality thresholds, annotations, reviewer decisions or missing observations.
**Synthetic evidence cannot receive public acceptance.**

An Accepted report records the owner's scoped decision. It is not a runtime
permission token and does not enable fallback by itself. Future execution admission
must check the actual current environment, dependencies, tier, expiry and
withdrawal state before constructing a private execution permission.

## Desktop and current environment

Use **Model evidence** in the desktop to list/inspect records, import a manifest,
review the supplied scope and record an explicit decision or withdrawal. Recovery
controls retry the original request and dismiss only acknowledged local entries.
Selection/status changes clear acceptance consent, and delayed replies cannot
replace a newer selection. These controls use the same verified Engine and private
recovery journal as the CLI. Headless state tests pass; rendered native desktop
qualification is still pending.

The [environment inspection command](PULSAR_INFERENCE_ENVIRONMENT.md) supplies
actual Engine/worker/named-file and typed host identities for an evaluation.
It does not verify complete runtime dependencies or authorize fallback.

## CLI

Use `pulsar model equivalence --help` for the command grammar. These operations
require the current authenticated local session and `model-pack-owner.token`;
the client reads that existing private capability without printing or journaling
its value. No project or project revision is involved.

```sh
pulsar model equivalence list --limit 8
pulsar model equivalence import /absolute/input.json --manifest-sha256 SHA256 --request-id ORIGINAL_ID
pulsar model equivalence status DOSSIER_ID
pulsar model equivalence withdraw DOSSIER_ID --expected-record-revision REVISION --reason 'Superseded evaluation' --request-id ORIGINAL_ID
```

Consult `accept --help` for the explicit measured-equivalence acknowledgment.
Importing a document is not acceptance, and this guide supplies no acceptance on
the user's behalf. A real evaluation and owner review are still required.

## Interrupted operations and withdrawal

Before sending a mutation, the CLI saves its original nonsecret intent and request
ID in a separate private recovery journal. Entries are bound to the original
session, credential identity and owner-capability digest. Only a validated matching
receipt acknowledges the operation. Do not create a new request ID merely because
the response was lost.

```sh
pulsar model equivalence recovery-list
pulsar model equivalence recovery-retry ORIGINAL_ID
pulsar model equivalence recovery-dismiss ORIGINAL_ID
```

Dismissal removes only an acknowledged local recovery entry. It does not withdraw
an Engine decision. A failed or uncertain operation remains available for explicit
same-ID retry. Import recovery can replay a retained Engine receipt even if the
original input file has since disappeared.

Withdrawal uses two transactions. The first durably reserves the original
request ID and records a pending withdrawal. The second updates the record and
saves its exact receipt atomically. A failed second transaction leaves withdrawal
pending across restart; acceptance is refused while it remains pending. Other
commands cannot reuse that request ID. Withdrawal can finish without a usable
wall clock, recording no invented timestamp. Acceptance checks a retained monotonic
wall-clock floor and the declared validity interval.

The first decision or pending withdrawal raises the database reader version to
35. Existing inner migrations preserve that outer version. Older readers must
refuse the database; deleting a marker or rewriting its version is not a downgrade
procedure. Reader checks, decision-to-receipt links and retained evidence are
validated before normal startup recovery.

## Verified software scope

The current checkpoint passes 1,651 Engine tests (101 ignored), 664 client tests
(14 ignored), 343 Protocol library tests, both strict lint modes, formatting,
architecture/source guards and app/worker builds. Six focused Engine regressions
exercise authenticated import/replay, Synthetic acceptance refusal, receipt-write
rollback, pending-withdrawal restart, original-ID conflict and withdrawal despite
corrupt evidence. The separate reader tests cover preserved migrations and rollback.

The actual isolated CLI campaign passes 15 command scenarios and checks the
preceding application refuses schema 35 while both the database and an existing
nonempty committed WAL remain unchanged. It uses only authored Synthetic evidence;
no model is executed, no measured acceptance is created, and no device is contacted.
All owned processes join and private scratch is removed. See the
[verification receipt](evidence/roadmap-2026-10-24/inference-equivalence/verification.json).
The Engine full suite used private RAM, so its pass does not qualify ordinary-host
storage latency. Earlier browser heartbeat and archive-journal failures remain open.

## Remaining work

This workflow has no automatic scheduler yet. The desktop panel is implemented,
with rendered native verification still open. It does not
qualify any installed model, CPU/GPU backend, real-media corpus or physical toy.
Runtime permission revocation, exact current-environment admission, bounded fresh
successor attempts, portable history and the actual quality/platform qualification
campaigns remain part of the full roadmap. See
[current status](PULSAR_CURRENT_STATUS.md) for verified results and open failures.
