# Offline component runtime updates

`runtime-update` publishes a complete new Linux x86-64 runtime candidate using changed supplied files and verified retained base assets:

```sh
pulsar runtime-update /absolute/delta /absolute/retained-base /absolute/new-installation \
  --expected-base-manifest-sha256 LOWERCASE_BASE_SHA256
```

A [single-file changed-payload archive](PULSAR_RUNTIME_UPDATE_ARCHIVE.md) is also supported by the same pinned update command. The delta directory contains the desired complete `runtime-candidate.json` and every changed asset. Omitted assets must have the same declared path, exact length and digest in the pinned base manifest. Each reused asset is copied and hashed independently. Present corrupt or symlink delta files refuse; they never cause fallback. Missing changed files, stale base identity, damaged reused bytes, incompatible generation-pack declarations and existing destinations refuse publication.

The same installer verifies space and byte/work bounds, private modes, full generation-pack correspondence and atomic no-overwrite publication. Interrupted stages use the existing inspection/discard workflow. Source and base manifest bytes are rechecked before publication. The result reports both manifest hashes, provided/reused file and byte counts and `qualified: false`.

The output is a complete candidate; it does not depend on hardlinks or later reads from the base. The base is preserved. Update does not start Engine, switch running processes, change model selection, copy credentials or launch inference. Choose a version explicitly with the existing `runtime-launch` command. Retaining an old application does not prove it can read data migrated by a new application; project-schema rollback compatibility remains a separate gate. No trust/counter/clock fact is rolled back.

Exact tests, predecessor installer regressions, native subjects and component-delivery results are recorded in `evidence/roadmap-2026-10-11/runtime-component-update/checkpoint.json`. Signed update trust, shared immutable-asset storage, quota/retention, active-job/power-loss, platform/native-window/human acceptance and actual Core/Complete qualification remain open. No candidate is promoted to a release or useful model qualification. The full roadmap remains PARTIAL.
