# Native timing for accepted live capture

Each capture epoch now keeps an immutable correspondence between its existing
`Instant`-relative ticks and the host's suspend-aware clock. It brackets the native
observation and retains both interval endpoints. Mapping never replaces the
original capture or acceptance tick with a later polling time.

The clock relationship is checked on ingress, completion, status and sweeping.
Regression or a changed relationship, including detected suspend, invalidates the
epoch and cancels its work. Retirement uses the last valid tick so a clock fault
cannot prevent cleanup. An initially unavailable suspend-aware clock preserves
ordinary preview; that epoch cannot later acquire a native timing stamp by rebasing.

## Preserve the original accepted pair

Only an Engine-accepted CPU result can retain a native stamp. It contains the two
original frame identities, source times and capture ticks, the original estimate,
capture and acceptance intervals, and the pairing continuity generation. Pending
frame replacement does not rewrite issued work. Source gaps invalidate prior
native stamps, including when older issued work completes afterward. Continuity
between results requires an exact shared frame, matching inference attempt and
profile, adjacent result sequence and unchanged pairing generation.

Stale completions and provisional CUDA output do not provide usable native
acceptance. Status reads neither renew the capture nor refresh original sample
age. Failure, retirement and finishing clear retained native acceptance.

This is timing metadata, not a device-dispatch capability. Existing source timing
meanings remain explicit: producer-declared capture, browser HTTP receipt and
decoded-worker delivery are not physical camera exposure. The bridge does not
qualify source correspondence, model output, motion bounds or device synchronization.

## Verification and next integration

The [checkpoint](evidence/roadmap-2026-10-23/hsp-capture-clock/checkpoint.json)
records exact checks and subjects. The focused live suite passes 94 tests (3 ignored),
including 11 new deterministic and Engine integration cases. Both strict lint
configurations, format/structure checks and app/worker builds pass. The full Engine
suite passes 1,436 tests (72 ignored) on controlled RAM with joined cleanup.

An actual CLI/Engine/worker campaign accepts CPU inference from an authored graph,
then verifies original Start/Continue replay after a planned Engine kill, retired
old capture state and fresh explicitly started capture. All owned processes settle;
the supervisor reaps exited descendants and needs no forced leftover cleanup.
These process checks use private RAM/network namespaces. They do not resolve the
preceding ordinary-host timing failures or qualify real-media/model/device behavior.

The next device path still needs native HSP preparation and clock synchronization,
original-request admission and controls, current source/qualification guards,
accepted motion-envelope checks, and direct delivery into the bounded HSP queue.
Browser analysis must run ahead of measured presentation. Useful model/media,
physical Handy, platform, six-axis, human and release gates remain open.
