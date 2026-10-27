# Self-contained Linux media tools

Date: 2026-10-09
Status: Authored decoder and native CUDA correspondence PASS; release/media qualification remains PARTIAL.

The closed CUDA runtime can use separate, pinned FFmpeg/FFprobe executables through the existing WorkerTools contract. No protocol, native-image ceiling, capture byte proof or lease changed in this checkpoint. The previous415 source records are unchanged; one build recipe is added.

## Source and build

The [official FFmpeg release source](https://ffmpeg.org/download.html) is version9.0.2. Archive SHA-256 is `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`. The detached release signature verifies against fingerprint `FCF986EA15E6E293A5644F10B4322F04D67658D8`. Original archive, key, signature and verification output are retained.

[Build recipe](../scripts/build-linux-media-tools.sh) accepts that pinned local archive and a new absolute output directory. It builds internal FFmpeg codecs/filters without autodetected external libraries, disables networking, and checks each executable has neither an ELF interpreter nor DT_NEEDED entries. The first build passes in71.47s. Candidate FFmpeg/FFprobe sizes are30,924,440/30,707,288 bytes. This is an x86-64 Linux candidate, not a Windows/macOS artifact or a qualified release bundle. Version/configuration, licenses and original source are retained.

```sh
bash /absolute/pulsar/scripts/build-linux-media-tools.sh \
  /absolute/ffmpeg-9.0.2.tar.xz /absolute/new-media-tools
```

The existing `PULSAR_FFMPEG` and `PULSAR_FFPROBE` environment variables select the candidate paths for a private campaign. They remain explicitly selected candidates; system tools and owner configuration are unchanged. Distribution integration and the full format/codec/platform matrix remain open.

## Decoder correspondence

Actual bubblewrap execution mounts only the two tools, authored source, proc/dev and temporary storage, with no host OS/library/linker-cache roots. The authored four-second FFV1 source yields32 frames and393,216 RGB bytes. RGB bytes, timestamp/packet hashes and FFprobe stream/frame metadata exactly match the installed9.0.2 tools; each frame hash is independently recomputed from its corresponding RGB bytes. The first checker incorrectly used DTS ticks as the RGB ordinal; this checker assertion and its correction to enumerate frames are retained. Decoder outputs already matched before correction. This proves the sampled FFV1/RGB/clock path, not broad format quality or latency.

## Actual production CUDA workflow

The unchanged `scripts/annotated-cuda-generation.test.mjs` passes without skips in79.58s using the current closed-runtime release, self-contained tools and the matching81-library cuDNN diagnostic policy. The authored model/source fixture is separate from user media. The test exercises the production Engine/worker path, generation and export, annotated preview, oversized-profile refusal, restart preservation of original CUDA evidence, portable project import/export equality and clone reopening after restart.

The actual receipt records27 successful CUDA runs and324 CUDA computation nodes. Maximum position difference from the authored reference is2.9802322387695312e-8. The generation program has28 timestamped actions and no gaps. Original and portable-clone exports are byte-identical. Final groups, retired custody, staging and creation records are empty; owned Engine exits are awaited by the fixture. The optional preceding-reader check is not enabled and remains unverified. No physical dispatch or quality qualification occurs.

## Limits and retained evidence

Evidence is in [self-contained-media-tools](evidence/roadmap-2026-10-09/self-contained-media-tools/). It binds416 source records, the exact release identity, build/static-link checks, decoder commands and hashes, native request/results/receipts and exported scripts. Private database and owner token are excluded. Architecture/source guards pass289/36.

This closes the demonstrated dependency gap for this explicitly selected native candidate. Default bundle selection, repaired desktop window/placed interval, cold live readiness/reuse, complete CUDA authority interleavings, real-media/model quality, Stash service validation, platform/offline packaging, Handy and human acceptance remain open.

The complete new build recipe passes in79.65s. Both rebuilt executables are byte-identical to the tools exercised by the native campaign. Wrong source hash is refused before output creation; shell syntax and diff whitespace checks pass. Owned scratch/build/source extraction is removed after results are retained.
