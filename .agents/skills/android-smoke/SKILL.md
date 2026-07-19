---
name: android-smoke
description: Boot the Android emulator, install/launch the three-rings app, attach Playwright over CDP (dev), or run the release-APK smoke (scripts/smoke-android.sh, phase-end only). Use for any Android verification — dev webview e2e attach, release smoke, emulator troubleshooting, or when adb/emulator state is unclear.
---

# Android smoke — emulator, attach, release check

Two modes. **Dev attach** is the per-task path (matrix path 1); **release
smoke** runs once at phase end (embedded-Axum coverage) via
[scripts/smoke-android.sh](../../../scripts/smoke-android.sh).

## Emulator boot (both modes)

```bash
adb devices                        # a "device" row? skip boot
emulator -avd Samsung_Flip_7 -no-snapshot-save -no-boot-anim &
adb wait-for-device
# poll until 1:
adb shell getprop sys.boot_completed
```

The AVD is host-side (Google ships no linux-arm64 NDK — none of this runs in
the container). First boot after host restart takes ~60–90 s.

## Dev attach (per task)

1. `cargo tauri android dev` from the **repo root** (beforeDevCommand's
   `cd ..` resolves against the invocation dir; from src-tauri/ it fails with
   "manifest path `Cargo.toml` does not exist"). It boots the leptos watch
   server, `adb reverse`s :3000, installs + launches the debug app.
2. The webview URL is `http://tauri.localhost/*` — Tauri proxies the dev
   server behind that stable origin; content is live devUrl content.
3. Attach (the socket name embeds the app pid — re-discover on every launch):

```bash
socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | head -1)
adb forward tcp:9222 "localabstract:$socket"
node end2end/android-cdp-check.mjs        # attach + evaluate sanity
```

4. Playwright: `chromium.connectOverCDP('http://127.0.0.1:9222')` → one
   context, one page. Navigate only with `page.goto('http://tauri.localhost/<path>')`
   (JS `location.href` destroys the execution context mid-evaluate).

## Release smoke (phase end)

`scripts/smoke-android.sh` — installs the release APK (embedded Axum, no dev
server), launches, waits, greps logcat for crashes, asserts the process is
alive. Debug-vs-release trap: **debug builds point at devUrl and skip the
embedded server entirely** — a debug APK "passing" proves nothing about
embedded-Axum; only release exercises it.

## Cleanup / gotchas

- The dev build injects a deep-link intent-filter into
  src-tauri/gen/android/.../AndroidManifest.xml — `git checkout` it before
  committing.
- `adb forward --remove-all` clears stale forwards after app restarts.
- Emulator unavailable → record "Android smoke deferred: emulator offline" in
  the task's spec Findings and flag the maintainer. Never silently skip.
