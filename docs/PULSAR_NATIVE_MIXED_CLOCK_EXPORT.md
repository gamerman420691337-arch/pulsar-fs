# Native multi-carrier source-time export candidate — 2026-10-09

Status: PARTIAL IMPLEMENTATION. Source compiles, but the actual mixed-clock export and fault controls remain unverified. This checkpoint is not release qualification.

Explicit native source-time export now derives each effective action from its independently verified producer and retiming chain. It no longer requires all contributions to match the selected producer's sampling configuration or a single mapper. The selected native anchor must still contribute effective actions, with exact original candidate, job, canonical program, receipt and placement identities. Every producer remains bound to the captured source through the existing bounded clock loader. Non-source-time publication retains the homogeneous ancestry guard; archival/mixed native-archival publication remains restricted.

Multi-carrier native proofs use schema 6. Each carrier retains its own ordered retiming chain; context retimings form a sorted unique catalogue of the referenced verified native receipts, not one global chain. Observation rederives actions, carriers, selections and mappings, verifies the selected anchor fields and requires the catalogue to match those references. Single-carrier native serialization remains schema 4. Legacy native schema 1/2 observation now rederives mappings too, so stripping ancestry and downgrading cannot bypass mapping checks. Legitimate legacy replay still needs an actual current-reader campaign.

The new authored fixture uses a real decoder and worker results with strides three and one, public candidate commit/merge, the production export seam, and actual observation. It checks original frames 4/5 at exactly 500/625 ms, plus four rehashed fault controls (mapping, carrier selection, anchor metadata, and ancestry-free legacy downgrade with a forged mapping). It contacts no Stash server or device. Its SQL setup uses synchronous OFF to isolate numerical/export behavior; it cannot qualify crash durability. Worker and production deadlines are unchanged.

Fixture development failures are retained: a stale candidate, merge boundary protection covering the entire interior selection, a source version that did not match Stash's content hash convention, and using the pairing token instead of the Stash owner capability. Earlier commentary's assumption that identical motion caused the merge refusal was corrected by inspecting boundary protection. The final fixture uses the root's exact anchor boundaries, real content version and proper host-local capability. An isolated predecessor copy uses the same corrected setup with the old production export guards. Neither side reached a completed verdict in the final 180-second comparison; both timed out. No predecessor clock refusal, successful current publication, or fault-control pass is claimed.

Default test compilation and architecture/source guards pass (294/36). The related ancestry group timed out at 40 seconds after one recorded pass; it is not a completed green group. The first CUDA compile used the default ort dependency without its CUDA feature and failed; the corrected CUDA compilation passed and its receipt is retained separately. Broad suites, native CLI/server campaigns, release packaging, placement/retiming combinations, legacy replay, portable correspondence and performance are pending. The previous release binary does not contain this implementation candidate.

Source inventory: 422 records, five existing paths changed, 417 unchanged; no new source module. See the [evidence packet](evidence/roadmap-2026-10-09/native-mixed-clock-export/checkpoint.json). Model/media, platform, physical Handy 2 Pro and human acceptance remain open. No training, private references, external service changes or physical actuation occurred.

## Runtime follow-up

The [runtime follow-up](PULSAR_NATIVE_MIXED_CLOCK_RUNTIME.md) reached a real unresolved-gap refusal in the authored fixture, corrected its unsupported trim endpoint, and added explicit gap-free preconditions. The corrected test compiled but timed out at 480 seconds; mixed-clock export and semantic controls remain unverified.


## Successor — 2026-10-09

[RPC error correlation and mixed-clock runtime](PULSAR_HANDY_RPC_ERROR_CORRELATION.md) records the corrected error decoder and the first completed mixed-clock export regression, including all four negative/restoration controls. Historical results above retain their original scope. Runtime reliability, public integration and the complete roadmap remain open.
