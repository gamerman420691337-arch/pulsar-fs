#!/usr/bin/env bash
# Build the pinned Linux media-tool candidate from a previously verified archive.
# This recipe is not a platform or media-coverage qualification gate.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo "usage: $0 /absolute/ffmpeg-9.0.2.tar.xz /absolute/new-output-directory" >&2
  exit 2
fi
archive=$1
output=$2
[[ $archive == /* && $output == /* && -f $archive && ! -e $output ]] || {
  echo "archive must exist; output must be new; both paths must be absolute" >&2
  exit 2
}
expected=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
actual=$(timeout 30 sha256sum "$archive")
[[ ${actual%% *} == "$expected" ]] || { echo "source archive hash differs" >&2; exit 1; }
scratch=$(mktemp -d /mnt/4tb/tmp/pulsar-media-tools-XXXXXXXX)
trap '/usr/bin/rm -rf -- "$scratch"' EXIT
timeout 30 tar -xf "$archive" -C "$scratch"
mkdir -- "$output"
cd "$scratch/ffmpeg-9.0.2"
timeout 120 ./configure --disable-autodetect --disable-shared --enable-static \
  --disable-debug --disable-doc --disable-network --disable-iconv --disable-xlib \
  --extra-ldflags=-static > "$output/configure.log" 2>&1
timeout 540 make -j8 ffmpeg ffprobe > "$output/build.log" 2>&1
for tool in ffmpeg ffprobe; do
  # Static archives alone do not prove a self-contained executable.
  timeout 10 readelf -l "$tool" > "$output/$tool-program-headers.txt"
  timeout 10 readelf -d "$tool" > "$output/$tool-dynamic.txt"
  if rg -q 'INTERP' "$output/$tool-program-headers.txt" || \
     rg -q 'NEEDED' "$output/$tool-dynamic.txt"; then
    echo "$tool still has dynamic runtime dependencies" >&2
    exit 1
  fi
  /usr/bin/cp -- "$tool" "$output/$tool"
  timeout 10 "$output/$tool" -version > "$output/$tool-version.txt" 2>&1
done
/usr/bin/cp -- COPYING.LGPLv2.1 LICENSE.md "$output/"
/usr/bin/cp -- "$archive" "$output/ffmpeg-9.0.2.tar.xz"
timeout 10 sha256sum "$output/ffmpeg" "$output/ffprobe" > "$output/binary-sha256.txt"
echo "Built pinned media-tool candidate in $output"
