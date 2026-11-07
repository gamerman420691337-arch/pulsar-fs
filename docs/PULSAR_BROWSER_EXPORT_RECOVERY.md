# Durable browser script-export recovery

Script preparation now persists one session/project-bound export intent in private
recovery storage before dispatch. It retains the inspected revision/master digest,
original request, application-owned output path, pinned directory identity and
comparison projection. The output directory is explicitly 0700 and its side
journal is 0600; the private registry is bounded and atomically synced. Existing
unbound legacy output directories are preserved and are not silently adopted.

After relaunching browser-live for the same project/session, press **Inspect or
recover script export intent**. Inspection reports the original request/revision
and acknowledgement without creating an export. **Retry original script download**
replays that request through current authenticated Engine authority. A different
prepared revision/digest refuses replacement. Startup never exports automatically.

Replay holds the private workflow lock, validates scope, credential binding and
application-owned direct-child path, then checks the pinned output directory before
dispatch. The original Engine acknowledgement is checkpointed. After acknowledgement
it reopens and validates the pinned directory, reads `stroke.funscript` relative to
that held descriptor with no-follow/nonblocking semantics, and rejects nonregular,
foreign-owned, shared-link or oversized files. Parsed bytes must match the inspected
master projection; confirmed byte digests are retained and checked on repeat.
No caller-provided arbitrary output path or secret reaches the page.

Subsequent [confirmed retirement/history](PULSAR_BROWSER_EXPORT_RETIREMENT.md) clears settled active pointers while preserving files. File reclamation and legacy orphan reconciliation remain required; saved
intents block replacement until explicitly settled. Broad filesystem/platform
crash qualification, recording recovery and useful-model/media/device/human/release
gates remain open. No toy command is created and all motion remains unqualified.

## Verification scope

The network-disabled native campaign prepares its registry before dispatch, drops
a real successful `ExportAxisPinned` reply and confirms exactly two exports (one
independent CLI export and one browser export). After an abrupt server kill and
restart, inspection reports the same unconfirmed original request. Replacing the
pinned parent with a new directory refuses without Engine dispatch; restoring the
original parent permits exact original retry and byte comparison. Three calls use
one request ID and create no duplicate export. Final custody is empty, all owned
processes/proxy join, and all 498 actions match the separate Engine export/oracle.

Rendered in-app browser QA records 898 estimates across six captures, explicitly
uses a 120-second project range, authors seven gap repairs, reviews/commits, then
loses the real successful browser export reply. After graceful server restart,
inspection recovers the same original request and explicit retry downloads 905
strictly ordered actions. Repeat produces identical browser bytes. Every position
is 38 and endpoints are 0 and 120000 ms. Prepared/recovered accessibility states,
proxy receipts, screenshot and actual downloaded files are retained. This is a
diagnostic scalar fixture; the authored tail is not observed video motion.

Clients pass 423 tests with ten ignored, fourteen mock-player cases and ten strict
validation commands pass, including both Clippy configurations, formatting, native
builds, Windows source compilation, architecture and guards. Targeted file tests
cover changed binding, out-of-storage paths, parent replacement, parent symlinks,
file symlinks and shared inodes. A held parent descriptor continues reading the
original file when its path changes. Initial privacy-permission and missed-builder
lint failures are retained; explicit 0700 construction repairs them without
weakening the checks. Core/Protocol/Engine sources are unchanged; no new full suites
are claimed for them. Source/runtime identities are reverified in the checkpoint.

No Engine-restart-after-lost-export or mid-registry-rename crash campaign is claimed.
Legacy orphan adoption, managed release, original recording recovery, broad
platform/concurrency, useful-model/media quality, arbitrary-tab capture, GPU live
six-axis, physical Handy sync and human/release acceptance remain open. Full roadmap
PARTIAL. See [checkpoint](evidence/roadmap-2026-10-13/browser-export-recovery/checkpoint.json)
and [ADR B3-199](../research/phase-b/ADR-B3-199-browser-export-recovery.md).
