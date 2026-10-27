# ADR B3-140: audio-modulated oscillation

Status: implemented; scoped Core/Protocol/Worker/native CLI checks PASS. Synthesized and unqualified; native rendered GUI and independent acceptance remain open.

Legacy audio-synth documents vibe/osc/oscillation but current consumers refuse them. Add an AudioMode::Vibe declaration and pure bounded Core synthesis using the existing strictly decoded RMS analysis. Expose all three modes in desktop and accept the historical CLI aliases. No new decoder, asset download, authority or physical playback is introduced.

For mapping offset O and span A, the carrier center is O+A/2, with half-span A/2 multiplied by RMS and sin(2*pi*t/125000000ns). Silence holds center. RMS is not peak-normalized or threshold-rescaled. Explicit spectral analysis gates quiet windows; omitted analysis retains raw full-band RMS. Use integer nanosecond modulo for carrier phase, preserve every feature timestamp and right endpoint, and refuse intervals above 31.25 ms (four samples/cycle minimum) rather than inventing interpolation. Worker produces its existing 10 ms windows. Disclose frequency, center/half-span policy, phase origin, feature/window uncertainty and uncompensated filter delay in lineage.

Compare independent quarter-cycle and native exported sample oracles, quiet/silent/band-rejected controls, sparse/invalid sequences and legacy Protocol modes. Verify the production Worker and shared CLI/desktop consumer seam. This is a deterministic oscillatory mapping, not musical beat tracking, legacy FFT parity, useful-motion quality or physical device admission. Native rendered GUI and all independent qualification gates remain open.
