#!/usr/bin/env bash
#
# Release-APK smoke against the Android emulator (host-side; see the
# android-smoke skill). Proves the embedded-Axum path: install, launch, wait,
# assert the process survived and logcat shows no fatal crash.
#
# Debug builds point at devUrl and never start the embedded server — only a
# RELEASE apk exercises embedded Axum. Do not point this at a debug build.
#
# Usage:
#   scripts/smoke-android.sh                 # build release APK, then smoke it
#   scripts/smoke-android.sh path/to.apk     # smoke an existing APK (e.g. CI's)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pkg="com.three_rings.dev"
avd="${SMOKE_AVD:-Samsung_Flip_7}"
apk="${1:-}"

say() { printf '\n== %s\n' "$*"; }

# -- emulator up?
if ! adb devices | grep -qE '^emulator-[0-9]+\s+device$'; then
  say "booting emulator $avd"
  emulator -avd "$avd" -no-snapshot-save -no-boot-anim >/dev/null 2>&1 &
  adb wait-for-device
  until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do
    sleep 3
  done
fi
say "emulator ready"

# -- apk
if [ -z "$apk" ]; then
  say "building release APK (aarch64)"
  (cd "$root" && cargo tauri android build --apk --target aarch64)
  apk=$(ls -t "$root"/src-tauri/gen/android/app/build/outputs/apk/*/release/*.apk 2>/dev/null | head -1)
fi
[ -f "$apk" ] || { echo "no APK found ($apk)"; exit 1; }
say "installing $apk"
adb install -r "$apk"

# -- launch clean
adb logcat -c
adb shell am force-stop "$pkg" 2>/dev/null || true
say "launching $pkg"
adb shell monkey -p "$pkg" -c android.intent.category.LAUNCHER 1 >/dev/null

say "waiting 15s"
sleep 15

# -- verdicts
pid=$(adb shell pidof "$pkg" | tr -d '\r' || true)
crash=$(adb logcat -d | grep -E "FATAL EXCEPTION|Fatal signal" | grep -c "$pkg" || true)
webview=$(adb shell "cat /proc/net/unix" | grep -ac webview_devtools_remote || true)

echo
echo "process alive        $([ -n "$pid" ] && echo "pass (pid $pid)" || echo FAIL)"
echo "no fatal crash       $([ "$crash" -eq 0 ] && echo pass || echo "FAIL ($crash)")"
echo "webview present      $([ "$webview" -gt 0 ] && echo pass || echo "warn (release webviews are not debuggable; informational)")"

if [ -z "$pid" ] || [ "$crash" -gt 0 ]; then
  say "SMOKE FAIL — recent logcat:"
  adb logcat -d -t 100
  exit 1
fi
say "SMOKE PASS"
