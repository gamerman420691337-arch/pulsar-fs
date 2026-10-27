# Close exact input before draining an acknowledged finite tail

Implementation verification is in progress; this decision establishes no physical
acceptance. Browser EOF can precede the visible playhead and the final inference
callback, so input closure is distinct from both Stop and CUDA finalization. Freeze
the exact final native ingress header, settle its original CPU work and delivery,
and confirm worker custody joined before exposing an opaque native input seal.
Serialized closure status describes that progress and cannot recreate the seal.

Keep input and playback lifetimes separate. An input seal rejects further points
but leaves ordinary age, lead and starvation checks in force while the final
Append is unsettled. Finite draining requires the exact terminal sample
acknowledged, actual Play acknowledged, no uncertain or active ordinary command,
and joined native writer/probe work. Preserve every original sample, command
window, Play reference, source lease and preparation clock ceiling. An unpaired,
stale or missing final result supplies no invented endpoint or holding point.

Use an optional, versioned finite-drain qualification extension. Absence retains
old subject bytes and conservative EOF retirement. Explicit selection must match
both reviewed device and source policy hashes, with conditional finite-tail and
input-closure evidence. Keep the existing stopping bound unchanged; new evidence
must establish its applicability from the actual final native Stop send gate.
A same-observation, outward phase projection covers dispatch allowance plus that
bound within the original source and clock lifetimes. Earliest-phase terminal
passage and the projected latest overrun must both satisfy the selected policy.
The resulting observation immediately latches the same owned Stop; late metadata
cannot defer it. A missed dispatch allowance invalidates normal-drain success but
cannot suppress best-effort stopping. Passage is neither a recovery drain proof
nor evidence of physical completion.

Phase evidence and irreversible passage follow the refinement in
[B5-068](ADR-B5-068-certified-passage-and-phase-uncertainty.md). A fresh interval can
widen without undoing earlier certified passage. Preserve monotonic native time,
reject intervals that contradict retained passage, and keep raw current/Stop
windows and their original authority. Current passage and future Stop projections
remain separate. Lead, starvation, overrun, terminal states and original command
facts/deadlines stay binding.

Browser closure preserves the visible player, timing and already queued preview
values. A final estimate returned by status retains its original submitted frame
capture time, and polling cannot renew it. Natural EOF, trim end and an outstanding
upload have explicit ownership; pause, seek, hidden page, source change, failure
and original expiry continue to abort. Passive capture retirement checks the
admitted timing lifetime, its original deadline and source revocation. Contention
while copying a phase snapshot, or a newer observation arriving after the reader
sampled its evaluation time, is not proof of lost authority. Exact observations
remain required at source binding and device dispatch; this distinction grants
no additional command authority and extends no expiry.

Source handoff and ongoing clock refresh
remain separate required work. See the [primary-source research](handy-hsp-sustained-timing-and-drain-2026-10-23.md)
and the [in-progress checkpoint](../../docs/evidence/roadmap-2026-10-23/hsp-finite-tail/checkpoint.json).
