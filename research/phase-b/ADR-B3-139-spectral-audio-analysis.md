# ADR B3-139: explicit spectral audio analysis

Status: implemented; scoped Core/Protocol/Worker/native CLI checks PASS. Native rendered GUI, real-media/music/device qualification remain open.

The audio-synth CLI defaults to bass but refuses every band except full. Add pure bounded Core band analysis, an optional checked Protocol declaration, Worker-owned strict decoding and lineage, and CLI/desktop controls. Existing Audio requests without analysis retain exact full-band RMS behavior.

Bands are bass 20–250 Hz, mid 250–2500 Hz, high 2500–8000 Hz, and full. Use causal second-order Butterworth high/low-pass stages with continuous f64 state, zero initial state, no extra PCM allocation, and coefficients from the [W3C Audio EQ Cookbook](https://www.w3.org/TR/audio-eq-cookbook/). Refuse unsupported sample rates; omit a low-pass whose cutoff equals Nyquist. No peak normalization. Explicit analysis gates RMS below the chosen threshold to zero and uses upward threshold crossings for onset pulses. Clamp RMS exceeding one and report saturation count. Preserve source sample timestamps and duration; causal frequency-dependent delay is uncompensated and disclosed. This is not FFT parity with the legacy implementation or musical beat tracking.

Keep bundle generation explicitly unsupported. Verify independent tone/DC controls, thresholds, invalid inputs, exact legacy behavior, Protocol compatibility, Worker lineage and native CLI generation. Retain real-media, rendered GUI, model quality, platform and physical Handy gates.
