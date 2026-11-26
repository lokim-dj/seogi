#!/bin/sh
# The spike's steps on the phone. Touches only the test app
# io.github.lokimdj.seogi.spike (its own package: Seogi itself and its
# recordings are never touched) and changes no phone setting.
#
#   tools/phone.sh check     the phone is there, unlocked, which model
#   tools/phone.sh install   install the test app and let it make its folders
#   tools/phone.sh push      models + practice clips into the app's folder
#   tools/phone.sh run [takes] [engines]   file test (default clean,noisy; all engines), waits, pulls
#   tools/phone.sh pull      results/ to eval/synthetic/out/dictation_phone/phone/
#   tools/phone.sh uninstall remove the test app (and the models in its folder)
set -eu
ADB=${ADB:-/opt/homebrew/share/android-commandlinetools/platform-tools/adb}
PKG=io.github.lokimdj.seogi.spike
ACT=$PKG/.BenchActivity
HERE=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
CACHE=${CACHE:-$HOME/seogi/spike-cache}
DIR=/sdcard/Android/data/$PKG/files
OUT=$ROOT/eval/synthetic/out/dictation_phone/phone

n=$("$ADB" devices | awk 'NR>1 && $2=="device"' | wc -l | tr -d ' ')
[ "$n" = 1 ] || { echo "phone not connected or not allowed (devices: $n). Plug in USB, unlock, allow USB debugging."; "$ADB" devices; exit 1; }

foreground() { "$ADB" shell dumpsys activity activities | awk '/topResumedActivity|mResumedActivity/ {print $NF; exit}' | tr -d '}'; }

case "${1:-check}" in
check)
  echo "model: $("$ADB" shell getprop ro.product.model) · Android $("$ADB" shell getprop ro.build.version.release) · SoC $("$ADB" shell getprop ro.soc.model)"
  echo "foreground: $(foreground)"
  echo "free: $("$ADB" shell df -h /sdcard | tail -1)"
  ;;
install)
  "$ADB" install -r "$HERE/build/outputs/apk/debug/spike-debug.apk"
  "$ADB" shell am start -W -n "$ACT" >/dev/null
  "$ADB" shell am force-stop "$PKG"
  ;;
push)
  "$ADB" shell mkdir -p "$DIR/models/zipformer-ko" "$DIR/models/qwen3" "$DIR/clips" "$DIR/results"
  "$ADB" push "$CACHE/zipformer-ko/." "$DIR/models/zipformer-ko/"
  for f in "$CACHE"/qwen3/*.gguf; do "$ADB" push "$f" "$DIR/models/qwen3/"; done
  "$ADB" push "$ROOT/eval/synthetic/out/dictation_phone/lines/." "$DIR/clips/"
  ;;
run)
  echo "foreground before start: $(foreground)"
  "$ADB" logcat -c
  "$ADB" shell am start -S -n "$ACT" --ez auto true --es takes "${2:-clean,noisy}" ${3:+--es engines "$3"} >/dev/null
  echo "running… (logcat SeogiBench)"
  "$ADB" logcat -v brief -s SeogiBench:I | while read -r line; do
    echo "$line"
    case "$line" in *"BENCH DONE"*|*"BENCH FAILED"*) break ;; esac
  done
  "$0" pull
  ;;
pull)
  mkdir -p "$OUT"
  "$ADB" pull "$DIR/results/." "$OUT/"
  echo "-> $OUT"
  ;;
uninstall)
  "$ADB" uninstall "$PKG"
  ;;
esac
