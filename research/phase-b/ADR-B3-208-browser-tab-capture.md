# ADR B3-208: explicit tab-capture input without source-player clock claims

Status: implemented; browser/platform acceptance open. 2026-10-13.

The browser player accepted local Blob media only. Live watching needs a separately
consented capture source while preserving bounded transport and clear timing scope.

Request getDisplayMedia directly from an explicit click, ask for video-only browser
surface and verify the returned track setting. The browser controls source choice;
preferences cannot restrict it, so refuse other/unknown surface types after selection.
Connect the stream to the existing muted video/frame path. Close owned tracks on stop,
revocation, source switch, page exit and failed analysis; fence late permission replies
with the existing playback generation. Never automatically retry permission.

Treat stream mediaTime as capture presentation time. It is not the source video's time
or an authenticated player clock; source playback/seek mapping and toy synchronization
remain separate contracts. Do not silently promote this input into useful learned motion
or physical qualification. Preserve the Engine protocol and no-device-output boundary.

The [W3C Screen Capture draft](https://www.w3.org/TR/screen-capture/) specifies fresh user
selection/activation and displaySurface preference/settings. Actual browser dialog and
platform acceptance remain open because isolated workspace tools were unavailable. See
[contract](../../docs/PULSAR_BROWSER_TAB_CAPTURE.md).
