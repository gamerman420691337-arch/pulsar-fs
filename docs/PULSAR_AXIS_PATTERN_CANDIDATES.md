# Explicit axis-pattern candidates

CLI and desktop can submit one through six explicit synthetic axis patterns to an existing project. Put one strict key=value pattern on each nonempty line:

```text
pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8
pattern=hold axis=yaw duration=2s frequency=0hz amplitude=0 offset=0.4
```

Use the current project revision:

```sh
pulsar pattern-candidate PROJECT /absolute/patterns.txt --revision REVISION
```

The file is bounded regular input; supported Unix systems refuse symbolic links and blocking special files. The shared client builder limits ASCII text to 24 KiB, one through six nonempty lines and one unique axis per line. Axes are stroke, sway, surge, roll, pitch and yaw. Each line uses the checked pattern grammar. All axes must share a duration and their combined action count must fit the existing one-million-action limit. Shared Core preflight now also runs at protocol admission before a generation job is accepted.

The CLI binds generation to the explicitly named revision and returns an uncommitted candidate. In desktop Generator, choose **Explicit axis patterns**, enter the same line format and choose **Generate axis-pattern candidate**. Review and commit through the existing candidate controls. Generation does not replace the master, export files, actuate devices or bypass edit protection. Existing explicit commit, selected-axis export, undo/redo and restart rules remain in force.

Every pattern action remains synthesized. These are deterministic authored motion patterns, without observed motion, free-form AI interpretation or image-semantic inference. They do not implement or qualify faithful visual six-axis recovery, all input families, useful tier models, physical devices or human usability.

Evidence is recorded separately in `evidence/roadmap-2026-10-11/axis-pattern-candidates/checkpoint.json`. Native controller/CLI results are distinct from rendered desktop and human acceptance. Full roadmap remains PARTIAL.
