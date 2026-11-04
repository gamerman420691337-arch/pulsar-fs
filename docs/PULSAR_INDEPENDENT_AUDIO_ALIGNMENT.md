# Separately captured audio with declared clock alignment

The production engine, CLI and desktop generation settings can use a separately imported audio source alongside video. Each source is captured into the existing immutable project snapshot store. The audio must be media in the same project and have a distinct source-version identity. Client declarations cannot supply worker filesystem paths.

```sh
pulsar generate VIDEO.mkv --mixed-audio mixed.json --audio-source AUDIO.mka --audio-alignment alignment.json --output OUTPUT.funscript
```

`mixed.json` contains the existing explicit intent and mapping:

```json
{"intent":"faithful","mapping":{"axis":"stroke","amplitude":0.2,"offset":0.4}}
```

`alignment.json` declares corresponding original source-clock instants:

```json
{"video_anchor":{"numerator":0,"denominator":1},"audio_anchor":{"numerator":1,"denominator":8}}
```

This means video time zero corresponds to audio time 0.125 seconds. A missing `rate` retains the original equal-rate mapping and JSON representation. To declare different clock rates, add a positive rational audio-seconds-per-video-second rate:

```json
{"video_anchor":{"numerator":0,"denominator":1},"audio_anchor":{"numerator":1,"denominator":8},"rate":{"numerator":1000001,"denominator":1000000}}
```

This rate declares one extra microsecond of audio clock advance per video second; after one hour the declared clock difference is 3.6 ms. It is supplied by the producer, not estimated or measured by Pulsar. The numerator must be a positive signed 64-bit integer and the denominator a nonzero unsigned 32-bit integer. Invalid rates and unknown rate fields refuse during typed deserialization before source import/admission. For every original rational video PTS, the worker computes `audio_anchor + rate * (video_time - video_anchor)`, subtracts the original decoded audio origin and selects the exact containing 10 ms RMS window. There is no independent zero alignment, intermediate timestamp rounding, automatic offset/drift estimation, repair of discontinuous decoded timestamps, resampling of video action times or hidden fallback. Signed clocks are supported; arithmetic overflow refuses. An audio time before decoded support or at/after its end has no contribution.

The CLI requires both source and alignment with mixed-audio settings. It imports both sources before submitting generation. Its settings file cannot name an existing project source; project APIs instead use `mixed_audio.independent.source_version` with the same typed `alignment`. Missing or foreign-project versions, evicted snapshots and non-media sources refuse. The engine resolves the declaration into a captured `WorkerRequest.independent_audio`, includes its identity in dependencies and uses the ordinary confined worker admission. The worker verifies audio bytes before and after execution and the launcher exposes only the captured file. Active queued/running/interrupted jobs retain the audio snapshot through the existing dependency eviction guard.

`faithful` preserves every available visual sample and its timestamp/evidence, even when audio conflicts. Audio only assists unavailable visual motion. `creative_audio` explicitly chooses synthesized RMS output; missing audio remains a gap and ordinary gap export restrictions apply. RMS mapping, visual sampling and review policy are unchanged from [container audio/video generation](PULSAR_MIXED_AUDIO_VIDEO.md). This is not semantic audio interpretation, beat tracking or physical motion reconstruction.

Receipts retain the independent source version/hash/length, clock anchors and optional rate, decoded original timeline, each original video time, aligned audio time, containing-window index, visual evidence and chosen contribution. Configuration and lineage bind the declaration and captured bytes. The desktop offers a distinct imported-media selection and rational clock anchors and an audio-seconds-per-video-second rate, retaining its existing 0.5–0.75 mapping. It rejects stale/same/non-media choices. Import both media sources using ordinary project import and select the video before generation. Controls and selection checks compile and pass; rendered usability is unverified.

Portable export checks that the historical captured audio identity has a matching media source payload in the selected project catalog. Import checks the same logical binding without opening or executing archived paths. Original manifest source IDs and declarations remain inert historical evidence under existing namespace maps; clone/re-export preserves them. This adds no database table or portable format and does not promise executable replay of archived paths or compatibility of a new manifest with an older worker.

## Local evidence and remaining gates

The [source-bound checkpoint](evidence/roadmap-2026-10-01/independent-audio/validation.json) records authored native CLI generation, faithful equality, synthesized creative values with unchanged action times, missing-support gaps, missing declaration/foreign-project refusal, supplier-file removal with preserved snapshots, portable clone/re-export/second clone and engine restart. Worker tests exercise nonzero audio origin, exact window correspondence, wrong captured bytes and foreign declarations. Protocol tests exercise signed rational mapping and overflow; authority tests protect active audio dependencies; portable capture tests reject a declared dependency that disagrees with the catalog; desktop tests refuse invalid selections. No real media, trained-model, physical device or human acceptance claim follows.

The [declared-rate checkpoint](PULSAR_INDEPENDENT_AUDIO_RATE.md) adds current source-bound tests, authored native campaigns and an independent exact-clock oracle. Declared constant-rate clock drift is now expressible without altering the visual lattice. GB-04 remains open for measured alignment, nonlinear or discontinuous clock correspondence, broader independent-source fusion, real-media faithful/creative comparisons and approved creative/human review criteria. GPU/six-axis/Handy, platforms, model quality and full roadmap/release qualification remain open. No training or sterile evaluation occurred in this increment. See [ADR B4-007](../research/phase-b/ADR-B4-007-independent-audio-clock-alignment.md).
