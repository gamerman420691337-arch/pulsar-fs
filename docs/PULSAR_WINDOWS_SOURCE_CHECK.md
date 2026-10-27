# Windows source and portable test checkpoint

**Windows GNU compilation PASS; emulated portable tests FAIL; native Windows qualification UNKNOWN.** This checkpoint advances the cross-platform build foundation and exposes remaining test/runtime scope. It does not qualify the required Windows MSVC distribution.

The retained offline commands are:

```sh
cargo check --workspace --all-targets --target x86_64-pc-windows-gnu --offline
cargo test -p pulsar-core -p pulsar-protocol -p pulsar-clients --target x86_64-pc-windows-gnu --offline --no-run
```

Both pass for the final subject. The first command initially failed because a Linux-only ignored WASM controller test compiled Unix permission calls on Windows. A subsequent check exposed optional source receipt/ancestry type inference through absent Unix branches and test helpers whose platform scope differed from callers. The corrected scope and explicit prepared-source type preserve Linux coverage and existing unsupported behavior. Cross-target compilation still emits target-specific unused/dead-code warnings; strict Windows lint is not established.

The second command emits 38 portable test executables. The repository diagnostic harness runs those exact executables under isolated Wine 11.19, with a private RAM prefix and disabled networking. It records **571 passed, 54 failed, 3 ignored** tests: 34 executables pass and 4 fail. Failures are Clients (22), Protocol library (23), annotated generation (1) and live CUDA report contracts (8). Many failing positive fixtures use Unix-style absolute paths that are not native Windows absolute paths. Two Client positives exercise retained package handling explicitly unsupported on Windows; a script destination comparison also ignores Windows canonical extended-path spelling. These failures remain open. Fixing fixtures must retain actual runtime path validation and explicit unsupported outcomes.

This is emulation, not native Windows execution. Pure/headless checks do not establish Windows named-pipe security, private filesystem guarantees, process confinement, media/inference/device behavior, rendered GUI, single-file packaging, offline tiers, signing or clean-machine operation. GNU ABI compilation does not establish MSVC compilation or native release acceptance. No Core/Complete edition or hardware qualification is claimed.

The [checkpoint](evidence/roadmap-2026-10-12/windows-source-check/checkpoint.json) retains initial build failures, final compiler output, binary identities and the full negative Wine campaign. Linux Core/Protocol suites and rebuilt Engine/Clients pass; the latter pass 841/322 with 70/8 ignored. Both strict Linux lint configurations, workspace check, formatting, native App/Worker build, 445 architecture checks and 36 source guards pass. An initial invocation used the older package-only Client executable; the final Client campaign uses the executable emitted by the current combined Engine/Client build. The Wine server is waited for inside its owned namespace and scratch is removed after reference checks. Shared caches/toolchains and borrowed runtime assets are preserved.

Architecture: [ADR B3-145](../research/phase-b/ADR-B3-145-windows-source-boundaries.md). Roadmap remains PARTIAL.
