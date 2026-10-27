# Acknowledgement-covered pending journal retirement

The shared Client service now offers explicit retirement of an inspected local
publication `.pending-UUID` file. The CLI command is:

```text
pulsar library-batch-retire-pending --publication-state /absolute/private/workflow \
  --pending /absolute/private/workflow/.pending-UUID --sha256 LOWERCASE_SHA256
```

The desktop Library batches panel exposes the same service under **Retire
recovered pending metadata**. Supply the original pending path and its inspected
SHA-256. This is a byte-preserving move into retained history, not deletion or
publication. The deterministic destination is
`.retired-pending-UUID-SHA256` in the same private workflow directory.

The current journal must contain completed positive publication acknowledgements.
The inspected pending journal must validate under the same recovery credential
and match the exact immutable batch/generation history, prepared identities and
per-axis request IDs. Previously recorded acknowledgements cannot regress.
Only acknowledgement/phase/error progression covered by the completed current
journal is allowed. Matching output files supply no recovery authority.
The operation dispatches no generation or export and leaves `record.json` intact.

On Linux/Android, the service holds the existing workflow lock and directory
handle, requires owned private single-link 0600 regular files, rejects symlinks,
bounds input to 1 MiB, verifies SHA-256 and current record/name identities, syncs
the held file, and uses a no-replace rename. It rechecks retained bytes and
identities before directory sync. A pre-existing destination refuses while
preserving both names. Other platforms explicitly report unsupported; Windows
source compilation is not native execution evidence.

Retry the **original path and digest** after uncertain rename/sync outcome.
If the original name is absent, only the exact deterministic retained file may
be inspected against the same covered history. Absence alone never means success.
A valid repeat reports `already_retired: true` and syncs again. Potentially changed
names return a typed uncertain result so the desktop preserves the captured
recovery action. Known preflight refusals remain editable. Every receipt carries
`qualified: false`.

## Verification and limits

The final Client suites pass 409 tests, with ten ignored. Both workspace strict
Clippy configurations, Windows source check (existing warnings), App/Worker and
controller builds, formatting, architecture and 36 source guards pass.
Engine/Core/Protocol sources are unchanged; their suites are not rerun here.
The initial two test failures came from missing private fixture-directory modes;
the failures and corrected rerun are retained.

The native Linux/private-RAM campaign kills the actual CLI client before its
acknowledgement-record rename, joins its tracer, restarts the owned Engine,
replays the original accepted axis ID without duplicate export, and completes
publication. Its genuine unrenamed pending journal is then moved without byte
change. Exact CLI and actual desktop-controller repeats preserve journal/output
bytes and job/export counts. The controller uses the same SessionClient recovery
credential as the CLI. An initial harness incorrectly expected a foreign
credential refusal; the successful move and failed assertion are retained.
A separate changed-credential journal verifies the actual refusal boundary.

Desktop positive retirement additionally uses a constructed duplicate of a
genuinely completed desktop publication journal. It is not desktop writer-death
evidence. Incomplete current acknowledgement, wrong digest, changed credential,
conflicting destination, malformed content and symlink cases refuse and preserve
bytes. Unit checks also cover hard links and nonprivate modes. Seventeen authored
outputs across four jobs match 101 timestamps from 0..1000 ms, position 45 before
500 ms and 55 thereafter. Existing Stroke/Roll/Pitch and unknown owner members
remain untouched; final counts are four projects/jobs/candidates and 17 exports.
Reader floor 34 is directly queried. Both owned Engines and the killed
client/tracer are joined.

This closes acknowledgement-covered metadata retirement only. Unknown effects,
missing positive receipts, general orphan collection, hostile same-owner writers,
power loss, other filesystem/platform guarantees, rendered GUI acceptance,
useful model/media quality, Handy hardware and human/release gates remain open.
Full roadmap PARTIAL; goal active.

See [ADR B3-184](../research/phase-b/ADR-B3-184-library-pending-retirement.md)
and [checkpoint](evidence/roadmap-2026-10-12/library-pending-retirement/checkpoint.json).


## Subsequent actual retirement interruption evidence

[The subsequent campaign](PULSAR_LIBRARY_RETIREMENT_CLIENT_DEATH.md) kills actual
CLI clients before and after the retirement rename, then verifies original-path
recovery through CLI and desktop after Engine restart. It leaves production
source unchanged and retains the qualification limits above.
