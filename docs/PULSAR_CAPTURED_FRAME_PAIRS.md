# Captured RGB pairs and bounded scheduling

Pulsar accepts one immutable captured RGB frame pair through the authenticated engine and confined generation worker. This is a functional input adapter and a separately tested scheduling kernel. It does not yet implement a continuously running camera/stream session or send device commands.

The source contains exactly 24,576 bytes: previous then current packed RGB frames, each 64 by 64 pixels with three unsigned byte channels. No header, stride padding, projection or container decoder is used. The source follows existing immutable snapshot, dependency pin, job, candidate, commit and export boundaries. The worker verifies bytes and hash, admits the fixed temporal CPU model ABI and produces one inferred script-position sample with a synthesized neutral anchor at the previous frame time (zero by default, or explicitly mapped as below). Review remains uncalibrated.

The declaration records `source_epoch`, strictly increasing `previous_frame` and `current_frame` identifiers, and `previous_time` and `current_time` as `{numerator, denominator}` rational seconds. Negative source origins are permitted. Their exact difference must be positive, at most 500 milliseconds and representable as a positive project nanosecond time. The worker floors the difference once and retains the quantization remainder. It never subtracts separately rounded absolute timestamps. These clocks and epoch are producer declarations; they do not establish measured acquisition or synchronization.

An existing temporal profile can be used as follows:

```sh
pulsar generate pair.rgb --model candidate.onnx \
  --inference-profile temporal-profile.json \
  --raw-frame-pair pair.json --output pair.funscript
```

The profile pins actual model/runtime/preprocessing/provider bytes. Ordinary finite-file generation remains available without `--raw-frame-pair`. Pair settings refuse file placement, selected-eye views, mixed audio, sample stride changes and non-stroke output. The receipt identifies `captured-rgb-pair-script-position-v1`, original clock declarations, exact interval conversion, CPU execution and review. It explicitly records `physical_dispatch: false` and `continuous_live_session: false`. A source snapshot bounds pixel bytes; a declared memory minimum is not an independently measured model-memory qualification.

The pure core `LiveScheduler` retains one in-flight metadata item and one latest pending metadata item. New pending frames replace earlier pending frames; the replaced metadata is returned so the acquisition owner can release corresponding bytes. In-flight work retains its original full epoch/project/revision/source/frame/artifact identity. Completion cannot relabel it with newer pixels. Expired work and retired completions are reported separately, and a mismatched token cannot clear current in-flight work. Retirement closes the scheduler permanently; resumption requires a fresh, unique epoch supplied by the engine.

Capture ticks and scheduler ticks are nanoseconds in one caller-owned monotonic clock domain. They are distinct from rational source time and project presentation time. Clock regression, future capture, changed identity and unordered source/frame clocks refuse without altering queue contents. Status reports replacement/expiration counts and accepted capture-to-analysis age. Optional added buffer is retained as declared policy (bounded to one second); the kernel neither implements presentation buffering nor subtracts it from measured age. It does not acknowledge physical stopping. Tokens are created by the scheduler and cannot be deserialized as authority.

Remaining GB-06 work includes engine-owned continuous acquisition, timestamp provenance, bounded pixel ownership, previous-frame continuity across drops, measured presentation-clock correspondence, scheduling integration, cancellation of non-preemptible inference, real capture-to-command timing and skips, GPU fastest-preset six-axis analysis, and qualified device admission/transport/response. The captured-pair primitive currently uses finite jobs; the kernel is not yet wired into a continuous engine session. Model accuracy and Handy physical qualification remain open. Sterile evaluation remains evaluation-only; this increment neither trains nor tunes against it.

Retained [validation and exact subjects](evidence/roadmap-2026-10-01/captured-frame-pairs/validation.json) include 704 passing Rust tests, seven ignored qualification cases, all 74 native-configured JavaScript cases without skips and 164 passing structural architecture checks. The authored pair receipt and exported actions are retained alongside raw gate logs. The CLI crosses the authenticated engine and confined worker, executes the fixed CPU fixture once, and refuses duplicate frame IDs, source intervals over 500ms and incorrect byte counts without exporting output. These checks do not establish trained quality, capture latency or physical response.

[Declared live presentation correspondence](PULSAR_LIVE_PRESENTATION_CLOCKS.md) now optionally places both captured frames in a shared project timeline and computes separate presentation due ticks. The clock-bound scheduler checks that mapping and preserves it in opaque work tokens. Mathematical buffer accounting is implemented; real playback buffering and synchronization measurements remain open.
