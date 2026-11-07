# B4-012: original-clock container audio and visual precedence

Status: implemented locally; GB-04 qualification remains open.

Mixed-media selection belongs in a pure per-sample core policy. Source decoding, timestamps, feature extraction, resource/deadline enforcement and immutable output artifacts stay in the isolated worker. The engine continues to own capture, authority, jobs and candidate publication. Clients declare intent and mapping rather than sending audio-derived edits directly.

Faithful mode retains every available visual result at its original sample time. This deliberately preserves uncertain visual inference too: uncalibrated confidence is not a sound authority for replacing it with audio. Only unavailable visual samples can receive synthesized audio assistance. Creative audio replacement is an explicit different intent; missing audio produces unavailable motion without implicit visual fallback. This implements precedence without claiming an approved creative-quality rubric.

The first production recipe reads both streams from one immutable container. Original absolute rational video PTS minus original audio origin selects exact containing RMS windows. Project alignment does not silently align stream origins, add onset timestamps or normalize audio amplitude. Audio selection never modifies the visual tracker's running estimate. Receipts retain both the visual result and the chosen contribution, plus clocks and review uncertainty.

Independent-source alignment, richer uncertain-section fusion, semantic audio, live input and real-media qualification remain required. The container recipe does not close those requirements. See the [production contract](../../docs/PULSAR_MIXED_AUDIO_VIDEO.md).
