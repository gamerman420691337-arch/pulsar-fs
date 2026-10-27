# Item 1 closure: portable-project checkpoint

Date: 2026-09-14

Status: **Implementation and local qualification complete; external review gate blocked by reviewer transport.**

## Current source

- Branch: `codex/pulsar-roadmap-m1`
- Last committed base: `4da4c08b59dc7b90e6c5aa3b420908257194eb8d`
- Current declared source fingerprint: `f705c4a960c0a64d9301842996c2d72107ecabae58ddc9f49c032e60c5a39ed1`
- Declared source entries: 210
- Current production binary: `target/debug/pulsar`
- Binary SHA-256: `864bd2fb48829e3ef85f9daddbdb646ffd96f020202973fde17bbdeab81fb31e`
- No commit or push performed.

## Local qualification

Command:

```sh
PULSAR_RUN_LARGE_PACKAGE_TEST=1 \
PULSAR_RUN_LARGE_PACKAGE_IMPORT_TEST=1 \
bash scripts/qualify-portable-desktop.sh
```

Result: exit 0.

- 563 passing Rust test executions across 38 suites; 0 failed; 7 ignored.
- 106 architecture checks and 36 architecture-scanner tests passed.
- Portable controller suite: 19 passed.
- Full client suite: 85 passed.
- Process target: 4 passed, including real-engine upload-ACK cancellation and relocation/re-export.
- Large export: 1 GiB-plus source/package streaming passed with bounded chunks and sampled RSS.
- Large import/re-export: 1 GiB-plus campaign passed with bounded control probes and sampled RSS.
- Actual-worker import qualification passed.
- TLC lifecycle/model checks passed, including expected negative-mutant violations.
- Existing compiler warnings remain; warning-free status is not claimed.
- No GPU, physical device, native-window, cross-platform, private-media or neural-accuracy qualification occurred.

## External review

Required `review_diff` retries were attempted after the P1D-008 fix, before and after full qualification. Results:

- `Transport closed`
- `Broken pipe (os error 32)`
- `Transport closed`

No current P1D-008 external verdict exists. Earlier seven-repair review returned primary **PASS WITH NITS**, but does not cover this patch. Independent local review found no confirmed defects in P1D-008. This remains a process/infrastructure blocker, not a code pass.

## Scope result

P1D-001 through P1D-008 are implemented and locally exercised. Historical failure receipts remain preserved. Good-phase B1-B6 work remains incomplete. Remaining implementation items 2-18 are ready to start, but item 1 should be marked fully closed only after external review returns a non-error verdict.

Raw current gate: `/home/brianklam/.p1d-repair-ReMRjb/item1-full-gate.log`.
