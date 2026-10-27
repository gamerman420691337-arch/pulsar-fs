# Offline assistant host setup

Status: implementation and final scoped verification PASS. Linux scope; full roadmap INCOMPLETE.

`pulsar assistant-setup` writes the existing private owner-policy format from measured local files. It runs before daemon discovery and does not create Engine state, run inference or change a project. Example:

```sh
pulsar assistant-setup --output /install/assistant.json --runtime /install/assets/llama-completion --model /install/assets/model.gguf --approve-host-inference
```

The output parent must already exist, be owned and not writable by other users. The output must be new. The default schema-2 policy requires runtime/model assets inside that folder and rejects symlinks. Use `--absolute-assets` deliberately for schema-1 absolute paths. The actual configured companion worker is measured; the policy will refuse a different worker at later startup.

Select the generated absolute policy path with `PULSAR_ASSISTANT_OWNER_POLICY` on the next Engine startup. Setup does not replace an active Engine or select a policy automatically. Host approval allows this asset declaration; inference requests and project effects retain separate approvals. Optional `--cpu-threads`, `--wall-time-ms` and `--worker-memory-bytes` must satisfy existing Engine admission. Defaults are configuration values, not qualified latency/memory promises.

Files are hashed using a 64-KiB buffer, bounded by Engine admission and checked for observable changes through held handles. Policy size stays at16 KiB. Publication uses exclusive no-follow creation through an owned held directory, mode0600 and file/directory synchronization. Existing output, invalid approval, unsafe paths/metadata and insufficient admission refuse. An uncertain write/sync may leave the newly created private file for inspection; inspect it and choose a new output path rather than overwriting.

This is a configuration component. It does not deliver Core/Complete, verify selected model/runtime compatibility, run useful learned inference, repair ordinary-storage latency, configure Handy secrets or qualify platforms/devices/human acceptance. [ADR B3-100](../research/phase-b/ADR-B3-100-local-assistant-host-setup.md) records the boundary.


Verification: final offline locked all-feature binaries build PASS in1m08s; default and all-feature strict workspace Clippy PASS in38.40/25.64s; formatting, architecture376 and source guards36 PASS. Complete selected default Engine/client library runs PASS765/219 with59/four ignored in26.37/8.39s under private2GiB tmpfs. The six policy tests are covered there; one isolated grammar test PASS on ordinary storage. A preceding ordinary six-policy-test invocation ends without a complete verdict at its40-second bound; the shell's eventual zero status is from the later grammar command, not a policy verdict. Storage reliability is not closed.

The actual rebuilt CLI runs under private tmpfs with networking disabled. Help, private measured schema-2 creation and no-Engine-state checks pass. Missing approval, a writable-by-others parent and existing output refuse; original policy bytes stay unchanged. Runtime/model inputs are authored one-byte metadata fixtures, not valid inference models. This demonstrates local configuration behavior, not compatibility, model quality or the required offline edition matrix. [Exact source/binary-bound checkpoint](evidence/roadmap-2026-10-11/assistant-host-setup/checkpoint.json) retains the probe and all gate limits.

A native restrictive-umask probe reproduces successful creation of a mode000 policy in the initial implementation. Publication now sets0600 through the newly created held file descriptor before writing; it never changes the process umask or an existing file. The same native regression now PASS with mode0600, success and no Engine state creation. Original mode000 failure is retained.

Final repaired-source verification repeats both strict lint configurations (13.97/7.48s), full selected default Engine/client libraries (765/219 PASS,59/four ignored,23.27/7.75s private tmpfs), formatting, architecture376 and guards36. Final all-feature binaries build PASS in47.25s. Both network-disabled native configuration probes PASS on those binaries. Runtime/model compatibility and useful inference remain untested by these authored metadata fixtures.
