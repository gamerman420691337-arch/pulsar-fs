# Authored H.264 orientation fixture

`rotation-h264.mp4` is a first-party generated FFmpeg `testsrc2` pattern, not sampled media. Four 96 by 64 yuv420p frames have source times 0, 1/2, 1 and 3/2 seconds. H.264 keeps decoder coverage when the selected bundled tool lacks the optional libx264 encoder. Rotation metadata is added at test time without re-encoding; the actual worker Decoder must preserve packed-source pixels and source time.

SHA-256: `779ae931551922845396f8fdc621874c45eb837531dadf36c4052322d346c6d6`. Size: 6336 bytes.

Authored with `/usr/bin/ffmpeg` n9.0.2:

```sh
ffmpeg -v error -nostdin -f lavfi -i testsrc2=size=96x64:rate=2:duration=2 -c:v libx264 -pix_fmt yuv420p -threads 1 rotation-h264.mp4
```

Byte reproducibility across encoder builds is not assumed. Update the identity explicitly if replacing the authored fixture. This fixture supplies interface evidence, not natural-media/model accuracy or platform/release qualification.
