# Spectral audio synthesis

`audio-synth` supports bass (20–250 Hz), mid (250–2500 Hz), high (2500–8000 Hz), and full-band analysis. The desktop audio generator exposes the same band and RMS threshold controls. [Vibe mode](PULSAR_AUDIO_OSCILLATION.md) adds an 8 Hz energy-modulated carrier through the same analysis path.

```sh
pulsar audio-synth source.wav --mode envelope --band bass --threshold 0.2 --output bass.funscript
pulsar audio-synth source.wav --mode beats --band mid --threshold 0.35 --output mid.funscript
```

Threshold must be finite and in [0.05,0.9]. Explicit analysis sets envelope windows below threshold to zero. Onset mode produces fixed pulses at upward RMS crossings that fit without overlap; omitted edge/overlap pulses are reported. It is not musical beat tracking. CLI defaults are bass/onsets/0.35; desktop defaults retain full-band envelope. Full band at the original 0.35 threshold retains the original ungated envelope. The existing audio-map command and Protocol requests omitting analysis retain original full-band behavior. Explicit six-axis recipes now use [audio bundle synthesis](PULSAR_AUDIO_BUNDLE.md); no automatic multiband recipe is implied.

Worker strictly decodes immutable media to calibrated 16 kHz mono PCM. Core applies continuous causal second-order Butterworth high/low-pass stages, computes 10 ms RMS windows without peak normalization, and clamps/report windows whose RMS exceeds one. High's upper cutoff is Nyquist at 16 kHz, so that low-pass is bypassed. Filters start at zero; no extra full PCM copy is allocated.

Window timestamps and duration retain the admitted source sample clock. Frequency-dependent filter delay is uncompensated; first-sample timestamps have analysis-window uncertainty. Lineage records options, cutoffs, algorithm, delay, gating, Nyquist bypass and saturation count together with source timeline and existing mapping/review evidence. Mapping remains synthesized and unqualified.

[ADR B3-139](../research/phase-b/ADR-B3-139-spectral-audio-analysis.md) specifies the boundary. The [spectral-audio checkpoint](evidence/roadmap-2026-10-12/spectral-audio/checkpoint.json) records four new Core controls, legacy/explicit Protocol compatibility, actual Worker lineage, 14 native authored-tone CLI workflows and four refusals. Final Engine/Clients suites pass 838/309 tests with 70/8 ignored. Both strict lint configurations, workspace check, formatting, 435 architecture checks and 36 source guards pass. The initial private-access test compile, range-style lint refusal and overly strict zero-leakage CLI oracle are retained with corrected results; production DSP was unchanged by the oracle correction. The first RAM probe retained its failure trace but its private temporary outputs expired with the namespace. Tone tests and native CLI execution do not establish real-media quality, legacy FFT parity, native rendered GUI usability or physical Handy qualification.
