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

- **`JAVA_HOME` must be a JDK the Kotlin compiler can parse (2026-08-15).**
  Android Studio's bundled JBR is now **25.0.2**, and AGP 8.11's embedded
  Kotlin throws `IllegalArgumentException: 25.0.2` out of `JavaVersion.parse`
  while configuring `:buildSrc` — gradle reports it as the cryptic
  `A problem occurred configuring project ':buildSrc'. > 25.0.2`, with the Rust
  half already built fine. Run the dev attach with the host's JDK 21 instead:
  `JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home cargo tauri android dev`.
- **A non-default `CARGO_TARGET_DIR` needs `mkdir -p target/site` first** — the
  Tauri build script resolves its `frontendDist` (`../target/site`) against the
  repo, not against the target dir, and panics with
  `resource path ../target/site doesn't exist`. Same directory the merge gate
  creates for the same reason.
- **Forward the socket that matches the *running* pid.** `head -1`/`tail -1`
  over `/proc/net/unix` picks a dead app's socket after a relaunch and the
  forward then lists no targets (curl just hangs):
  `adb forward tcp:9222 localabstract:webview_devtools_remote_$(adb shell pidof com.three_rings.dev)`.

- The `three-rings://` deep-link `<intent-filter>` in
  src-tauri/gen/android/.../AndroidManifest.xml is **committed and
  load-bearing** (WB-01M0640EKXM1QCBMFG7K97E4M7) — it's what lets the Google
  OAuth Android bounce page's `three-rings://auth/callback` deep link reach
  the app; without it "Open the app" silently does nothing. A dev/build run
  re-injects the identical block (byte-for-byte, including the trailing
  whitespace on its blank `<data>` placeholder lines) between its own
  `<!-- DEEP LINK PLUGIN. AUTO-GENERATED. DO NOT REMOVE. -->` markers, so
  `git status` should come back clean afterward — a real diff there is a
  signal something changed (e.g. `tauri.conf.json`'s `plugins.deep-link`
  config), not routine noise to discard. **Do not `git checkout` this
  block.** If some *other*, unrelated line in the manifest (or elsewhere
  under gen/android) picks up incidental dev-run churn, that's still
  fair game to revert — just leave the deep-link filter alone.
- `adb forward --remove-all` clears stale forwards after app restarts.
- Emulator unavailable → record "Android smoke deferred: emulator offline" in
  the task's spec Findings and flag the maintainer. Never silently skip.

## Release APK signing in worktrees

`src-tauri/gen/android/keystore.properties` is gitignored, so a fresh
worktree does NOT have it — and gradle then silently falls back to the
auto-generated **debug** keystore. A debug-signed APK refuses to install
over the release-signed app on a device (`INSTALL_FAILED_UPDATE_INCOMPATIBLE`,
forcing an uninstall that the persistent-keystore design exists to avoid;
this bit the maintainer twice on 2026-08-16). Before ANY release APK build
in a worktree, copy it from the main checkout:

```sh
cp <main-repo>/src-tauri/gen/android/keystore.properties src-tauri/gen/android/
```

Then verify the built APK's signer is the release key, not `CN=Android
Debug`: `apksigner verify --print-certs <apk> | grep "certificate DN"` —
expected DN contains `O=threeringsdev`. An APK handed to the maintainer
must ALWAYS be release-signed.
