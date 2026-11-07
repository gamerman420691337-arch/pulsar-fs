---
status: proposed
---

# Retain certified passage without rewriting fresh phase evidence

Rendered run016 exposed a false regression in one forward-only HSP stream.
Native time advanced from 102248034046617 to 102248034099945 ns while the phase
interval changed from [10999,11056] to [10998,11056] ms. The new interval overlaps
the preceding evidence; its lower endpoint alone does not prove playback reversed.
The original lower-endpoint rule from B5-062 stops on this admissible uncertainty
change. The [actual trace](../../docs/evidence/roadmap-2026-10-23/hsp-continuity/rendered-browser-hsp-ui-016/test.redacted.log)
remains failed evidence, not an accepted demo or physical qualification.

The stream kernel owns both the raw current interval and an independently retained
certified-passage floor C. For one unchanged forward-only stream at nondecreasing
native time, an earlier certified passage remains known. A fresh valid interval
[L,U] is consistent only if U >= C. Its intersection with that existing fact is
[max(C,L),U]. This is evidence intersection, not a tolerance, rate extrapolation,
revised observation, renewed lease or claim of physical completion.

Keep raw HSP phase/reference facts intact. Native time regression and a new upper
endpoint below C remain refused, preserving terminal behavior while running or
draining. After validating a new context, advance C only
to max(C,L). Irreversible ring release uses C and retains the existing preceding
endpoint rule. Fresh raw upper bounds continue governing minimum lead, starvation
and projected overrun. Keep maximum-future checks on the raw lower bound as a
conservative choice. No rejected point, retired stream, foreign identity, gap,
expired sample, invalid window or lost authority becomes admissible from C.
Original Play bytes, server time, issued effects and their original deadlines
remain unchanged. Current Engine authority is always required independently.

A validated source transfer preserves the stream zero and cumulative coordinate,
so it preserves C. A genuinely new stream initializes an empty passage history.
Passage is in-memory evidence owned by that native stream; journal replay, process
restart and historical status cannot restore it as playback authority.

Finite drain keeps current passage distinct from future Stop projections. Never
credit a projected future lower endpoint as current passage. Retain a separate
projected floor and its nondecreasing target time for compatibility of successive
Stop projections; refuse a new projected upper endpoint below that floor. Current
terminal passage uses only current certified passage. Each raw Stop window still
comes from the same selected observation as its raw current window, must cover the
unchanged Stop budget inside original lifetimes, and must satisfy its fresh upper
overrun bound. Original seal identity, drain expiry, actual Play acknowledgment,
terminal Append acknowledgment and joined ownership remain required. Store raw
windows in observations; do not rewrite them to look more precise.

Required checks cover the actual overlapping-window case; unchanged predecessor
retention; raw lead/horizon and starvation limits; contradictory windows and native
clock regression; preserved passage through source transfer; fresh-stream reset;
finite current versus projected separation; projected contradiction/time reversal;
original seal expiry and overrun; and unchanged original Play/effect facts. Run the
full Core/Engine checks and native/rendered streaming scenario after integration.
Useful-model, physical-device, human and release gates remain open. This decision
refines B5-062's lower-endpoint rule; it does not change qualification acceptance,
source/clock expiry, or B5-067's selection-before-sampling contract.
