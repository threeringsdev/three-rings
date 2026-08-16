# Real-WKWebView probe harness

Diagnostics, **not** part of any build, test tier, or CI job. Nothing in the
workspace references this directory; it exists so a claim about "the desktop
`.app`'s webview" can be *measured* instead of guessed at.

## Why it exists

Playwright's `webkit` project is a WebKit **build of its own**, not the system
WebKit that `WKWebView` — and therefore the Tauri desktop shell — actually
runs. Twice now (`#148`, `#163`) a popover bug was reasoned about from
screenshots plus a Playwright-webkit run, and twice the conclusion was wrong:
`#163` concluded the system WKWebView lacked CSS anchor positioning and
rebuilt the JS fallback around that. It doesn't lack it — see
specs/app-ui.md's `WB-01M064BMRF8QBKAYJ4C9CNGQ0H` finding, which this harness
produced. macOS only (it links `WebKit.framework`).

## Build

```bash
swiftc -O -o /tmp/wkprobe end2end/wkwebview/wkprobe.swift
```

The binary loads a URL into a real `WKWebView` in an offscreen window,
injects a driver script at document-end on every navigation, and prints
whatever the page posts back:

```
window.webkit.messageHandlers.probe.postMessage(json)  -> stdout, then exit 0
window.webkit.messageHandlers.log.postMessage(str)     -> stderr
window.webkit.messageHandlers.shot.postMessage(name)   -> <shotdir>/name.png
```

`shot` is awaitable — the harness calls `window.__shotDone(name)` once the PNG
is on disk. Await it, or the snapshot races whatever the driver does next
(learned the hard way).

```
wkprobe <url> [driver.js|-] [width] [height] [timeoutSeconds] [shotdir]
```

## The two probes

**1. Engine capabilities, no app required** — `CSS.supports` for every
declaration `popover.rs` emits, a read-back of how the engine actually
*parsed* that rule, and a live measurement of real anchored popovers against
real triggers (does anchoring move them, and where):

```bash
/tmp/wkprobe file://$PWD/end2end/wkwebview/anchor-support.html - 1280 800 30
```

**2. The app's own pickers** — signs in, opens the selection tray's
`Move to…`, and measures the panel, its `Command` body, the `CommandInput`,
and what the top layer actually hit-tests to. Needs a dev server
(`cargo leptos watch`) and `E2E_EMAIL` / `E2E_PASSWORD`, which the harness
reads from the environment and hands the page as `window.__E2E` — never
hard-coded here:

```bash
set -a; source end2end/.env; set +a
/tmp/wkprobe http://127.0.0.1:3000/login \
  end2end/wkwebview/app-picker-driver.js 1700 1050 200 /tmp/shots
```

Probe at a realistic window size. The desktop `.app`'s 800×600 default is not
representative of overlay adjacency (maintainer note, 2026-08-16) — 1700×1050
is the size to reproduce at, though both were checked for this finding.

**Chromium half** — `chromium-compare.mjs` runs the same two questions through
Playwright's chromium so "per engine" claims have both engines behind them:

```bash
cd end2end && node wkwebview/chromium-compare.mjs
```

## Verifying against the built `.app` — read this first

**Two `three-rings` bundles on one Mac are indistinguishable to AppleScript.**
Every bundle this repo builds carries `CFBundleIdentifier com.three-rings.dev`
and the executable name `three_rings` — the maintainer's installed
`/Applications/three-rings.app` and any `target/release/bundle/macos/`
build from a worktree included. System Events resolves a process by that
identity, not by pid, so with two instances running:

```applescript
tell application "System Events"
  set p to first process whose unix id is <the pid you built>
  set size of (first window of p) to {1700, 1050}   -- may hit the OTHER instance
end tell
```

reports success, reads the new geometry back, and applies it to whichever
instance the Apple Events layer picked. This burned a whole verification round
(2026-08-16): a fix was measured against the maintainer's older, unfixed
`/Applications` build and declared broken. `set frontmost` throwing `-25208`
is a symptom of the same collision.

**So never verify by app name.** Either:

1. Quit every other `three-rings` instance first (check with
   `pgrep -lf "MacOS/three_rings"`), or
2. drive by pid. `axdrive.swift` does this — `AXUIElementCreateApplication`
   takes a pid and cannot alias:

```bash
swiftc -O -o /tmp/axdrive end2end/wkwebview/axdrive.swift
swiftc -O -o /tmp/winid   end2end/wkwebview/winid.swift
/tmp/axdrive <pid> resize 80 60 1700 1050
/tmp/axdrive <pid> raise
/tmp/winid   <pid>            # CoreGraphics truth: window id + real bounds
```

Confirm the resize landed with `winid` (CoreGraphics is keyed by pid and never
lies) before trusting any click or screenshot. `screencapture -l<windowid>`
then captures that exact window regardless of z-order.

**Cheapest identity check of all** — ask each running instance what it serves.
Each `.app` runs its embedded Axum on a dynamic port:

```bash
lsof -nP -iTCP -sTCP:LISTEN -a -p <pid>
curl -s http://127.0.0.1:<port>/catalog | rg -o 'data-name="Command" class="[^"]*"'
```

A build carrying a given fix serves markup that proves it. That one line
settles "which build am I actually looking at?" without any GUI at all, and is
worth running before every visual verification.
