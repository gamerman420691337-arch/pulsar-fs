# Audio and video generation

Video generation can explicitly use the audio stream in the same immutable source container. The engine's existing source, job, candidate, revision, protected-region and confinement APIs remain authoritative. No audio stream is consulted unless mixed audio is declared.

```json
{
  "intent": "faithful",
  "mapping": {"axis": "stroke", "amplitude": 0.2, "offset": 0.4}
}
```

Save that declaration as `mixed.json`, then run:

```sh
pulsar generate INPUT.mkv --mixed-audio mixed.json --output OUTPUT.funscript
```

`faithful` preserves every available visual sample, including uncertain visual inference and explicit anchors. Conflicting audio cannot change its time, position or evidence kind. Audio supplies synthesized motion only at sampled times where visual motion is unavailable. `creative_audio` explicitly replaces visual output with the audio mapping. In that mode, missing audio stays unavailable rather than silently reverting to video. Both intents require an explicit mapping; omitted or unknown intent refuses.

This recipe is full-band mono RMS, decoded at 16 kHz in 160-sample windows. Position is `offset + amplitude * RMS`; silence holds offset, and quiet audio is not expanded to full range. It is not musical beat tracking, semantic audio interpretation or observed physical motion. Sampling occurs on the video's existing frame lattice; this recipe does not add audio-onset action times.

Each original rational video PTS is compared with the original decoded audio origin. Selection floors the exact elapsed rational value to its containing 10 ms audio window. Negative elapsed times and times at or after the decoded audio end have no audio support. Streams are not independently aligned to zero, and project placement, trimming or reverse playback does not alter this source-clock lookup. Audio timestamp discontinuities, unsupported clocks, missing streams and exceeded resource/deadline limits refuse through the worker. There is no timeline repair or hidden fallback.

The immutable configuration and receipt retain intent, mapping, original stream clocks, decoded audio timeline, visual position/evidence, selected audio-window index and selected contribution for each video sample. Audio selections remain `synthesized` and review-flagged. Original visual uncertainty remains in the review evidence even when creative output chooses audio. Missing combined support remains a motion gap and ordinary gap export restrictions apply. The neutral visual tracker retains its own position; audio output cannot feed back into later visual estimates.

The desktop generation panel offers container audio and explicit creative replacement. Its current mapping is 0.5–0.75. The CLI accepts the complete checked mapping declaration. Desktop controls compile; manual usability is unverified.

Local evidence uses authored textures, constant PCM and an authored ONNX wiring fixture. It tests conflicting audio, unchanged faithful motion/times, explicit synthesized creative output, missing intent refusal and a nonzero audio stream origin. These are functional software checks, not trained-model, real-media, creative-rubric or device qualification. See the [validation receipt](evidence/roadmap-2026-10-01/mixed-audio-video/validation.json).

[Separately captured audio with explicit unit-rate alignment](PULSAR_INDEPENDENT_AUDIO_ALIGNMENT.md) now reaches the engine, CLI and desktop settings with captured identities and portable historical payload checks. GB-04 remains open for measured alignment/drifting clocks, broader evidence fusion and real-media faithful/creative comparisons. Semantic audio interpretation, approved creative thresholds, six-axis reconstruction, live inputs and human review-effort evidence remain separate roadmap requirements.
