# ADR B3-209: close inaccessible tab captures without stopping a newer source

Status: implemented with scoped mock and native transport regression evidence. 2026-10-13.

The new tab input handled ended tracks but missed temporary source inaccessibility.
A post-play rejection also occurred after generation advancement, leaving an attached
stream outside the original generation's cleanup. Fixing that by comparing stream
identity exposed a second case: null identity could match a late permission rejection
and clear a newer local playback.

Stop owned sharing on mute, reject already-muted/ended selections, and keep explicit
reshare as the only resume path. On failure, cleanup if a non-null selected stream
still belongs to this share, or if the permission generation remains current. Never
match null stream identity as ownership. Keep existing frame/reply generation fences,
Engine leases and no-device-output boundary. No protocol or Rust source changes.

Mock regressions exercise the failures at their real player handlers. Real browser
mute/autoplay/dialog behavior, source-player clocks and device acceptance remain open.
See [contract](../../docs/PULSAR_BROWSER_TAB_LIFECYCLE.md).
