# Declared live presentation correspondence

The pure live scheduler and captured-pair generation worker now share a checked source/project/presentation mapping. This supplies software correspondence and functional CLI output placement. It does not attest synchronization, acquire camera pixels, buffer a player, run a continuous inference session or dispatch Handy commands.

`LivePresentationClock` declares one immutable epoch and three corresponding anchors: original rational source seconds, signed project nanoseconds, and unsigned monotonic presentation nanoseconds. Mapping is unit rate. Optional added buffer is bounded to one second. Each source timestamp is mapped from its exact difference with the original source anchor; the complete project expression is floored once. The original clock, source values and fractional nanosecond remainders are retained. There is no accumulated previous-frame rounding, implicit zero-origin assumption, guessed drift correction or playback-rate change.

Presentation due time is the presentation anchor plus the source difference and optional buffer. Buffering changes due time only; it does not move project actions, change inferred positions or reduce capture-to-analysis age. Signed source offsets before the anchor are supported when resulting monotonic ticks remain representable. Underflow, overflow, excess buffering, changed epoch, negative emitted project times and quantization collisions refuse. Scheduler/project time and physical response remain distinct quantities.

The optional `presentation_clock` field in the captured-pair declaration is:

```json
{
  "epoch": "the-same-source-epoch-as-the-pair",
  "source_anchor": {"numerator": 10, "denominator": 3},
  "project_anchor": 500000000,
  "presentation_anchor": 100000000,
  "added_buffer_ns": 1000000000
}
```

Here a previous source timestamp of 10/3 maps to project 500,000,000 ns and a current timestamp of 11/3 maps to project 833,333,333 ns with the exact fractional remainder retained. The buffered current presentation due tick is 1,433,333,333 ns plus its remainder. Removing the buffer changes the due tick by one second and leaves the exported project action times unchanged. Without this optional declaration, the existing pair adapter retains the relative previous-frame origin.

The declaration follows `pulsar generate pair.rgb --raw-frame-pair pair.json` through existing immutable source capture, authenticated project authority, strict model/runtime profile admission, confinement, candidate acceptance and export. The worker maps both the synthesized neutral anchor and inferred current action. Review uses the mapped project interval. Its receipt carries original pair declarations, both presentation mappings, project action times and the separately quantized original source interval. The historical `current_quantization` field remains the original interval; `pair_interval_quantization` states that meaning explicitly. `presentation_buffer_executed` remains false because this adapter calculates due times but does not buffer or schedule playback.

`LiveScheduler::with_presentation_clock` additionally binds the clock epoch and buffering to admitted scheduler identity/policy. Each offered frame must have exactly the mapped project time. Source time, frame ID and project time must all increase, including after quantization. Replacing pending work retains the immutable mapping in the issued opaque token; later pixels cannot change it. Accepted status records capture-to-analysis age and presentation lateness separately. Buffered due times never allow an expired analysis to become current. Revision/source/epoch switches still require retirement and fresh authority.

Native functional coverage uses an authored deterministic ONNX wiring fixture, exercises actual CLI/engine/confined-worker inference, verifies offset project action export, checks that buffered and unbuffered exports are identical, and refuses changed epoch, buffering over one second and negative mapped output. Pure tests cover negative origins, fractional rounding, monotonic underflow/overflow, changed clock admission, mismatched project times and quantization collisions. This is software evidence, not model-quality or latency qualification.

Remaining work includes engine-owned continuous acquisition and byte ownership, measured acquisition clocks and player correspondence, real presentation buffering, sustained inference scheduling, non-preemptible cancellation, per-axis/GPU recipes, calibrated capture-to-command observations, actual Handy admission/transport and physical response. A producer's timestamp declaration cannot close these gates.

The retained [validation checkpoint](evidence/roadmap-2026-10-01/live-presentation-clocks/validation.json) binds current source and native binaries to 708 passing Rust tests, seven ignored qualification cases, all 74 passing native-configured JavaScript tests without skips and 166 passing structural architecture checks. Raw logs, original relative/offset buffered/offset unbuffered receipts, native program bytes and exports are retained. Buffered and unbuffered project exports have identical bytes; their declared due ticks differ by one second. Native build and diff checks passed. This increment leaves the privately trained candidate and previous negative sterile evaluation unchanged.
