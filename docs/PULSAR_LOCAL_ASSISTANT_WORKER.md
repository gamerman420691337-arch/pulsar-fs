# Local assistant: learned worker and second candidate

Date: 2026-10-10. Status: **PARTIAL application delivery; actual learned worker verified in bounded Linux probes; full roadmap INCOMPLETE**.

## What now executes

The dedicated Pulsar worker executable recognizes `assistant-worker` as a data-only role. It refuses extra role arguments and unconfined entry, reads at most 32 KiB of strict request JSON, validates bounded host context/scope, attempt identity and declared model/runtime identities, then hashes actual mounted model/runtime bytes before spawning native inference. Runtime and model paths are fixed inside the worker namespace. User/model text never becomes an executable path, shell command, driver action or arbitrary settings key.

The worker runs pinned-style noninteractive CPU inference with a separate system message, 2,048 context tokens, at most 256 generated tokens, explicitly bounded threads and one deadline spanning verification, native launch and response. It creates private prompt/schema files in its parent's private tmpfs. Native stdout is bounded to the proposal body plus the exact tested EOG trailer. Nonblocking output/diagnostic handling checks the deadline throughout; failure kills and polls the direct native child. Unconfirmed whole-tree retirement and admission belong to the parent, which must retain cgroup/resource custody. This role does not itself implement the Engine parent or prove process-tree admission release.

A schema offers all actions available in the supplied context, plus NoAction. Target/review/edit references and sample strides are enumerated from that host scope; unavailable action variants are omitted. The model chooses the action. The strict protocol decoder independently checks the complete JSON body against the same scope. A request-bound response contains the original version/attempt/runtime/model and proposal. It authorizes no project, source, evidence, native exception, generation setting, edit or device effect.

The worker's code is compiled into the existing binary-only worker composition. No new product library, dependency, database schema, Command/Response variant, framing version or client-native runtime import was introduced. The first-party desktop assistant control is still disabled. Host-supplied context remains a declaration: fresh authenticated bindings and effect authorization must come from the future broker.

## Actual second candidate

Official Qwen3-0.6B Q8_0 was fetched without authentication from `Qwen/Qwen3-0.6B-GGUF` at revision `23749fefcc72300e3a2ad315e1317431b06b590a`. The API's publisher metadata and downloaded bytes agree: 639,446,688 bytes, SHA-256 `9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031`. The license copy is retained. Sources: [pinned publisher model card](https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/blob/23749fefcc72300e3a2ad315e1317431b06b590a/README.md), [pinned publisher metadata](https://huggingface.co/api/models/Qwen/Qwen3-0.6B-GGUF/revision/23749fefcc72300e3a2ad315e1317431b06b590a?blobs=true). The browser metadata fetch failed; the unauthenticated Python API request succeeded and checked revision/size/hash before download. Final hash was independently verified again after retaining the assets.

The unchanged CPU llama.cpp candidate from [the first checkpoint](PULSAR_LOCAL_ASSISTANT_CANDIDATE.md) was reused: source `b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c`, executable SHA-256 `f332257812cf358954a1701c9fb205e5ac50b4fbdf0e2c09944cc25ea58459ae`. SSE4.2/BMI2 and dynamic system-library requirements still apply; this is not a supported-platform release package.

| Probe | Expected proposals | Result |
| --- | ---: | --- |
| Separate system context, unrestricted reference strings | 6 | FAIL 5/6: invented review ID 1234567890 |
| Same requests/context, host-scoped reference schema | 6 | PASS 6/6 |
| Actual built Pulsar role inside Linux namespaces | 6 | PASS 6/6, original response identities preserved |
| Repeat role campaign plus refusal checks | 6 + 6 refusals | PASS 6/6 and 6/6 |
| Follow-up distinguishing prelaunch and native deadlines | 6 + 7 refusals | PASS 6/6 and 7/7 |
| Actual production worker adapter through an explicit pinned-model Rust test | one target request | PASS, 2.57 seconds |

Requests cover selecting the second cup, refusing an absent elephant, selecting a recorded review flag, changing sample stride, proposing a selected-range smoothing recipe and refusing device movement. These are synthetic context/proposal results, not application effects or actual recorded media. The unrestricted-reference failure is retained. Offered IDs avoid invented references; they cannot guarantee user-intent accuracy, safe edits, truthful host context or protection against prompt injection. The candidate is promising for further evaluation, not selected or statistically qualified as a bundled default.

Observed standalone request times were 2.36–3.21 seconds with unrestricted references and 2.86–5.63 seconds with scoped references. Role probe ranges were 3.01–3.39 and 3.23–3.93 seconds. Each request reloads the model, with warm filesystem/cache and other builds running. These are recorded execution times, not a comparative performance benchmark, whole-application latency or peak-RSS qualification.

## Refusal and isolation evidence

Real compiled-role probes refuse unconfined entry, extra CLI role arguments, an invented approval field, 32,769-byte input, wrong model digest and a 300-ms deadline. Every case exits nonzero, emits no proposal and exposes its expected diagnostic. The short-deadline case returns in 0.307 seconds while hashing, before native launch; its diagnostic is `assistant whole-worker deadline exceeded`. A follow-up separately uses a two-second deadline: the native pipe monitor reports `worker wall deadline exceeded` in 2.029 seconds, exercising the post-spawn failure/cleanup path. That campaign passes six positive and seven refusal cases. No device or project effect occurs.

The namespace harness uses bwrap unshare-all, a private PID/network namespace, read-only system-library mounts and read-only worker/runtime/model bindings, private 16-MiB tmpfs, minimal explicit environment, outer process-group timeout and 2-GiB RLIMIT_AS. It bounds both parent pipes and records exact arguments. This successfully exercises actual native inference within those namespaces. It is a development harness, not the authenticated Engine's durable worker fence, immutable capture or aggregate/cgroup pool. Ambient host libraries are not a captured dependency closure; request hashes do not establish immutable source custody. Python is evaluation tooling, not a required installed runtime.

## Software gates

Final targeted worker tests PASS: two passed, one real-model case ignored by default. That explicit pinned real-model case was then executed separately and PASS. Complete ordinary-ext4 all-feature engine library PASS: **725 passed, 80 ignored, 11.61 seconds**. Protocol library **112 PASS**; workspace strict Clippy both default/all-feature all-target configurations PASS; formatting PASS; architecture **354 PASS** and source guards **36 PASS**. The earlier worker build overlapped source edits; its initial targeted result is retained for chronology but the later rebuilt targeted result and exact built-role binary are the current subjects.

A wire-contract run reached its 120-second deadline while sharing the build lock and compilation; no executed wire tests were claimed from it. The rerun PASS: contracts 10 and two frozen-wire tests covering three fixtures. Exact logs are recorded in the [source/binary-bound packet](evidence/roadmap-2026-10-10/local-assistant-worker/checkpoint.json). Complete client/workspace runtime and broader platform/rendered/media/device/human gates were not rerun; earlier results remain their own subjects. Full engine regression success does not establish assistant application functionality.

## Retained files and next work

Source lives in `crates/pulsar-engine/src/assistant_worker.rs`, protocol request/result/scope types in `crates/pulsar-protocol/src/assistant.rs`, and bounded experiments in `scripts/evaluate-local-assistant.py` and `scripts/qualify-assistant-worker.py`. Exact logs, failed/successful requests and schemas, worker/runtime/model identities, source snapshots and checksums are in the evidence packet. The second model/license/manifest are retained under `/home/brianklam/Projects/Funscripts/assets/pulsar-local-assistant-comparator-2026-10-10/`; the prior runtime remains in its first candidate asset directory. Own build/download/test scratch is removed after terminal handles and retained-byte checks. Shared cache and upstream/services remain untouched.

Next implement the Engine parent with immutable sealed assets, shared admission/fence/cgroup custody, cancellation/status/recovery and fresh-context approval. Bind model-selected target/settings/review/edit recipes to actual source/view/project revisions, expose recorded review reasons/confidence and apply edits through protected durable history/undo. Then wire first-party controls and bundled offline assets. Generation pause/checkpoint/assistant/resume, broader adversarial/quality evaluation, platform/install/first-launch, actual Handy and human acceptance remain open. No annotations exist; none were needed or fabricated for these software/synthetic probes, and no useful training or real-media quality claim is made.
