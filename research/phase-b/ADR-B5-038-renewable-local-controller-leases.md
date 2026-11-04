# ADR B5-038 — Distinct renewable controller lifetime with owned expiry

Acquire controller ownership separately from durable paired-session authentication and
nonplaying preparation. Explicit consent and fresh program/authority checks precede local
arming. Each lease has an Engine-issued controller ID, generation and native deadline.
Renewal is a generation-checked mutation; replay observes existing state and cannot extend or
reconstruct a lease. Expired, released or restarted leases require a new preparation/acquisition.

An owned wakeable monitor holds the native registry, not the Engine or project database, and
is stopped and joined before Engine resources retire. Database reads and commits must not
hold that registry. Linux deadlines use CLOCK_BOOTTIME, including suspend; clock reversal or
failure retires authority and invalidates that Engine's controller clock domain. Missing or
stopped monitor authority is rejected. Keep unexpected physical stopping uncertainty and its
native stop intent rather than dropping them during retirement.

Local Armed state does not enable physical playback. Server/device/browser clock integration,
physical dispatch and stop reconciliation, platform support and actual qualification remain
required. Lease duration is an authorization policy, not a measured physical stopping bound.
