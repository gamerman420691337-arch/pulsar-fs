# Native candidate verification experiment, 2026-10-09

Status: PARTIAL; production experiment restored to its exact prior source. No startup improvement or native handoff qualification.

## Hypothesis and code

Native capture originally hashes each copied temporary library and then hashes the assembled image before returning it. The experiment used private provisional candidates named by the validated declaration, retained the same bounds, source-change checks, FICLONE/fallback copying, readonly publication, fsyncs and final full-image hash, and left both later mounted-byte proofs unchanged. Candidate names and metadata were never authentication. General artifact snapshotting was unchanged.

Two new tests demonstrated that matching candidate names/metadata cannot admit wrong bytes and that a preexisting corrupt candidate is neither overwritten nor authenticated. Direct experimental default/CUDA Engine builds pass621/623 with54/68 ignored. Architecture/guards pass289/36. These are experiment checks, not a final-source workspace gate. The experimental CUDA Cargo release builds in3m51s and its standalone inspector in1m57s, with25 Engine warnings. No model or device qualification is implied.

## Results and decision

The actual live campaign FAILS with `false !== true`: the first capture is observed retired/inactive at about30.06s without Ready. At29.03s the exact `native-capture-worker` is still copying, with roughly1.828GB written; the sampled published prefix has66 distinct libraries/73 aliases totaling1.317GB. Of28 native-worker samples,19 wait in dirty-page throttling. No memory-observer or live-worker exec role is sampled. Full image verification and reuse are not reached.

The standalone contract test passes authored21-byte capture with independent readback and worker-slot release, wrong-hash refusal, symlink/FIFO refusal, pre-cancellation and invalid-deadline refusal. Its full75-library,1,945,994,000-byte capture returns `native capture absolute deadline exceeded` at60002ms under the inspector's existing60s budget, so the overall test FAILS (`refused` versus expected `captured`). Large-image deadline/cancellation subcases after that assertion are not reached. Every completed inspector case confirms cleanup and storage_after_cleanup=0. The live final fence is empty and its owned Engine terminates by SIGTERM.

These unmatched runs do not prove a causal performance regression. They also provide no verified startup benefit from removing the duplicate read. The added private candidate helper and its two tests are therefore restored from the prior source archive after verifying no concurrent change to that path. The exact415-record restored source equals the preceding native-image-reuse-runtime checkpoint. The previous shared-image implementation remains; no unrelated source is modified.

The [packet](evidence/roadmap-2026-10-09/native-candidate-verification/checkpoint.json) retains the415-record experimental source, patch, build/test logs, observations, process outcomes, restored manifest and experimental executable identity. That executable is explicitly not promoted; the retained pre-experiment supplier remains the source-matching supplier. Scratch copies, test executables and owned run roots are removed after evidence retention.

## Next action and remaining scope

Before another copy-path change, measure the producer's writeback, memory/cgroup pressure and source/destination I/O under an owned copy workload. Preserve unrelated workloads and global settings. Remove no byte proofs, increase no live lease, and do not infer a cause from a wait-channel sample alone.

Cold readiness, native shared-image handoff, unresolved workspace failures, complete live/encoded/camera/six-axis behavior, model/real-media quality, installed Stash, offline/platform matrix, physical Handy and human/release gates remain open. Full roadmap objective unchanged.
