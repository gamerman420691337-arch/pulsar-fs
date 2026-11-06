# Inspect the current inference environment

`pulsar model equivalence environment --profile /absolute/profile.json` reads a
declared inference profile and reports the current Engine, worker, named tools
and host observations. Use this when preparing or checking the environment
identities in an equivalence evaluation. The command requires the authenticated
local session and existing model-owner capability.

The Engine must already have the profile's exact runtime configured. An explicit
CPU profile selects its configured CPU owner-policy runtime when present, even
when the default runtime differs. The report also verifies those policy library
files and includes the policy/image identities. Runtime
discovery happens when the Engine starts; replacing a file or changing a launch
environment variable does not refresh an existing Engine's pinned identity.
Use the current development executable described in the
[quickstart](PULSAR_APP_QUICKSTART.md).

## What the report establishes

The version-1 report contains the exact submitted profile and its canonical hash,
plus these observations:

- **Engine:** the loaded process image read through the kernel's `/proc/self/exe`
  link, which remains distinct from the worker image.
- **Worker and named dependencies:** current bytes at the retained worker,
  FFmpeg, FFprobe and runtime paths. Lengths, SHA-256 values and descriptor/path
  metadata must still match the Engine's pinned identities.
- **Platform:** Linux architecture and kernel release/build. The platform identity
  is the hash of a versioned, canonical observation document.
- **Hardware:** CPU model, topology, microcode, features, current CPU affinity and
  kernel-reported usable physical memory. Canonical ordering makes equivalent
  observations comparable. CPU frequency, free memory and uptime are excluded.
- **Configured CUDA device, when requested:** kernel-checked UUID/PCI/minor and
  driver observations, plus verified named library bytes. This path does not load
  an inference runtime or test CUDA execution.

The collector reads bounded root-owned procfs information and compares repeated
samples. Changes during collection, replaced named files, mismatched runtime
bytes, unsupported parsers or an elapsed observation deadline cause refusal.
The current native CPU collector supports Linux x86_64. Other platform coverage
remains unfinished. Hostname, domain name and machine serial numbers are omitted.

The command records no equivalence decision, job, inference admission or fallback.
Its wall-clock timestamp describes when the observation completed; it does not
renew an evaluation's expiry or reserve the observed hardware for later use.

## Limits relevant to fallback

Every report sets `report_only: true`, `qualified: false` and
`runtime_dependency_closure_verified: false`.

The named dependency list is not proof of the complete runtime dependency set.
In particular, the ordinary CPU worker currently uses read-only host library
mounts. Kernel/CPU observations and a matching ONNX library cannot establish the
identity of every library that execution may load. The [captured CPU runtime](PULSAR_CAPTURED_CPU_RUNTIME.md),
fresh resource/authority checks and a consuming attempt's lifetime are still
needed before qualified automatic fallback can use an accepted comparison.

The model identity in the submitted profile remains a declaration: this inspection
does not establish that its model file is installed, executable or accurate. Actual
model execution, heldout quality measurements, owner acceptance and physical-device
qualification remain separate requirements.

## Desktop evidence review

The desktop's **Model evidence** tab exposes the existing equivalence workflow:
refresh/list records, inspect a selected record, import a manifest, record an
explicit owner decision, withdraw an exact revision and recover an original
request. File access, capability reads, journal operations and Engine calls run on
the desktop worker rather than in the rendering callback.

Acceptance requires a separately fetched current record, a named operator and
fresh consent. Selecting another record or receiving a replacement status clears
that consent. Synthetic evidence and pending withdrawal cannot be accepted.
Delayed replies cannot replace the current selection. Recovery retries preserve
the original request ID; dismissal removes only an acknowledged local entry.

This tab shows historical review state. It does not arm a device, qualify a model
or enable automatic fallback. See the
[equivalence workflow](PULSAR_INFERENCE_EQUIVALENCE.md) for evidence requirements,
withdrawal persistence and CLI recovery.

## Verified scope

The preceding environment checkpoint passed 1,664 Engine tests (101 ignored), Clients pass 678
(14 ignored), and Protocol passes 356. Both strict lint modes, formatting,
architecture/source guards and app/worker builds pass. The actual CPU CLI campaign
checks two live host observations and their typed canonical identities, refuses
changed runtime declarations and changed configured file bytes, and leaves work,
admission and equivalence tables unchanged. All owned processes join and temporary
state is removed. See the
[verification receipt](evidence/roadmap-2026-10-24/equivalence-runtime/verification.json).

The campaign uses its own nonexecuted media-tool files to test byte correspondence;
it does not establish media-tool functionality, useful inference, CUDA hardware
support, full dependency coverage or physical-device behavior. The desktop checks
are headless state/render checks, not an exercised native window. Prior browser
heartbeat and archive-journal contention failures remain unresolved.
