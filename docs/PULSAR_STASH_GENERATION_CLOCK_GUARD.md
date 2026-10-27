# Live Stash generation clock guard

Live Stash publication now checks the recorded clock of every contributing generation, in addition to existing source ancestry and retiming-chain checks. Each bounded typed job manifest must match its job, project and source binding. Captured source identity, exact placement specification and any raw-frame clock declaration must match the selected origin. Placement instance IDs, creation revisions, generation presets and motion values do not define clock correspondence. Authored candidates continue to inherit their recorded bases; archived generation restrictions remain unchanged. Historical paths are metadata and are never opened by this check.

## Reproduced defect and correction

The preceding implementation replaced a candidate's job ID with a mere present marker and checked its source version and retiming history. Two jobs on one captured source could therefore merge different placement rates while both had an empty retiming chain. Publication then mapped the entire master through the selected job's clock.

An authored production-interface campaign merges synthesized audio motion generated with a 2x source/project placement into a 1x master. At the interior project point 250 ms, the contributing receipt identifies source frame 4 at 500 ms. The old publication writes position 48 at 250 ms and its proof incorrectly claims source frame 2 at 250 ms. The [independent counterexample](evidence/roadmap-2026-10-09/stash-generation-clock/independent-counterexample.json) verifies this 250 ms discrepancy from retained original receipts, proof and script bytes. The 125 ms merge boundary preserves the parent value and is not used to prove misattribution.

The same native test is red against the preceding executable and green against the rebuilt executable. Two independent generation jobs with identical declared placement specifications remain publishable, even though their placement IDs and values differ. The differing-clock case now returns DependencyMismatch and creates no sibling script; both branches preserve the master. Invalid, missing and oversized origin metadata also refuse. No persistent schema or protocol changes are required.

## Verification and limits

The [source-bound checkpoint](evidence/roadmap-2026-10-09/stash-generation-clock/checkpoint.json) records one changed production source, one new native test and 416 unchanged records among 418. Current default and CUDA Engine suites pass 632/634 tests with 55/69 ignored. Offline locked CUDA release build and seven-file runtime assembly pass. All runtime bytes and the three retained successful publication scripts match their recorded identities. Architecture checks pass 289 and source guards pass 36. Protocol/client source is unchanged; preceding 99/137 test results are inherited, not rerun.

The existing native source-clock publication campaign also passes forward/reverse export, verified retiming, restart, replay, two portable clones, archived publication and its rejection cases. Initial fixture setup errors are retained separately from the actual red counterexample. Successful outputs are copied before the next fixture deletes its own sibling. Nine owned fixture Engines are terminal with empty custody; private credentials and databases are not retained. Scratch is removed after sealing the packet.

The installed Stash configuration declares port 9999, but no local listener was observed. These campaigns use authored media and a private GraphQL fixture; no user library, real Stash service, training, sterile evaluation or physical device is used. They establish neither useful model quality nor live-server/platform/device qualification. Existing successful export receipts remain historical records; the guard does not rewrite their scripts or timing proofs.

Mixed-clock publication still requires explicit piecewise correspondence, including contribution boundaries, retiming histories, source domains and output collision/gap handling. The full roadmap remains partial, with model/media, live/six-axis, assistant/plugin, offline/platform, physical Handy and human/release gates open.
