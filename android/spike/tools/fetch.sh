#!/bin/sh
# Downloads what the spike needs into places git ignores:
#   android/spike/libs/sherpa-onnx-2.0.3.aar   (Apache 2.0)
#   $CACHE/zipformer-ko/                       Korean streaming Zipformer, int8 (Apache 2.0)
#   $CACHE/qwen3/Qwen3-ASR-0.6B-{Q8_0,Q4_K_M}.gguf  (Apache 2.0), the desktop catalog's revision
# CACHE defaults to ~/seogi/spike-cache (outside the repository).
set -eu
HERE=$(cd "$(dirname "$0")/.." && pwd)
CACHE=${CACHE:-$HOME/seogi/spike-cache}
mkdir -p "$HERE/libs" "$CACHE/zipformer-ko" "$CACHE/qwen3"
get() { [ -s "$2" ] && return 0; curl -fL --retry 3 --speed-limit 50000 --speed-time 60 -o "$2.part" "$1" && mv "$2.part" "$2"; }
get https://github.com/handy-computer/transcribe.cpp/archive/refs/tags/v0.4.2.tar.gz "$CACHE/transcribe.cpp-v0.4.2.tar.gz"
get https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.3/sherpa-onnx-2.0.3.aar "$HERE/libs/sherpa-onnx-2.0.3.aar"
Z=sherpa-onnx-streaming-zipformer-korean-2024-06-16
if [ ! -s "$CACHE/zipformer-ko/tokens.txt" ]; then
  [ -d "$CACHE/$Z" ] || curl -fL https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/$Z.tar.bz2 | tar xjf - -C "$CACHE"
  cp "$CACHE/$Z/encoder-epoch-99-avg-1.int8.onnx" "$CACHE/$Z/decoder-epoch-99-avg-1.onnx" \
     "$CACHE/$Z/joiner-epoch-99-avg-1.int8.onnx" "$CACHE/$Z/tokens.txt" "$CACHE/zipformer-ko/"
fi
REV=e4e16599b900eb0cb36e524514756bb92eb092b7
for q in Q8_0 Q4_K_M; do
  f=Qwen3-ASR-0.6B-$q.gguf
  [ -s "$CACHE/$f" ] && [ ! -e "$CACHE/qwen3/$f" ] && ln "$CACHE/$f" "$CACHE/qwen3/$f"
  get https://huggingface.co/handy-computer/Qwen3-ASR-0.6B-gguf/resolve/$REV/$f "$CACHE/qwen3/$f"
done
echo "ready: $HERE/libs and $CACHE/{zipformer-ko,qwen3}"
