---
status: proposed
---

# Select original observations before sampling a fresh evaluation time

A [retained native diagnostic](../../docs/evidence/roadmap-2026-10-23/hsp-continuity/clock-source-diagnostics/confirmed-cause.json)
sampled `93884748511057 ns`, then selected a browser observation received at
`93884748771825 ns` and expiring at `93885248766809 ns`. The receive time was
`260768 ns` later than the sample. The fixed-time read correctly refused that
observation for the earlier tick; its fresh clock-refresh caller then treated the
refusal as permanent source-binding failure. The diagnostic run passed four native
scenarios and failed one. It confirms the ordering race, not a validated repair.

For a fresh observation evaluation, retain the exact active source, its immutable
browser timing publication and its native playback-clock pin before requesting
one fresh native clock sample. Evaluate those selected inputs at that sample and
return the evaluation tick together with the facts or snapshot. Fresh consumers
and preparation wrappers must use the returned tick, without taking another sample
or substituting a timestamp obtained before selection. Selection failures cannot
be hidden by resampling.

Keep ordinary current-facts evaluation distinct from snapshot evaluation. Both
share this ordering, but only the snapshot path requests the finite-tail projection
when required. Ordinary facts must not fail solely because future Stop coverage is
unavailable. A finite-tail snapshot derives current and projected facts from the
same selected observations and clock pin, with the future time measured from that
one evaluation tick. Wrappers delegate the corresponding operation without adding
a projection or sampling again.

A fresh reader that selected publication A may finish using A after publication B
appears, only while A's original receive-time, expiry, source and lifetime checks
remain satisfied. This retains an already-selected observation; it is not a search
for a previous publication after refusal. Replacement still retires a captured-start
token. Binding, source cancellation, poison and actual clock-regression checks remain
mandatory. This refines [B5-065](ADR-B5-065-native-timing-publication.md) without
changing its distinct fresh-reader and captured-start lifetimes.

Fixed-at APIs and final guarded command claims keep their existing semantics.
Earlier effects retain their original references, selected pins, deadlines and
custody. Refresh retains its original until bound and requires the predecessor
observation to remain current; synchronization age/ceiling and the explicit
continuity duration are not restarted. No blanket Busy result, retry policy,
previous-publication fallback or grace interval is introduced. Expiry during
selection/evaluation still refuses the operation and preserves owned stopping.

Required deterministic regressions, not passed claims:

- Select A, publish B inside the sampling callback, and assert exactly one sample
  and an evaluation of A within A's original bounds. Also replace the clock pin
  between selection and sampling; the selected generation must retain its limits.
- Advance that callback to A's original expiry while B remains valid. Refuse A
  without fallback, renewed deadlines or a second clock sample.
- Cancel the selected source/lifetime or return a clock error/regression during
  sampling. Preserve refusal; do not convert it to contention or retry custody.
- Keep fixed-at reads before the latest publication's receive time refusing, and
  keep old captured-start tokens retired after replacement.
- Verify ordinary current-facts evaluation does not demand finite-tail projection,
  while the snapshot path checks the original future Stop budget using the same
  selected inputs and returned evaluation tick.
- Exercise the actual watchdog, clock-refresh and preparation-wrapper paths.
  Check progress under publication interleaving, refusal at real bounds and
  unchanged original effect/pin identities, predecessor expiry, refresh until,
  synchronization ceiling and continuity duration.

Implementation and repaired native/composed verification remain pending. These
requirements and the existing failing software evidence establish no browser/display,
model, physical-device, stopping, independent acceptance or release qualification.
