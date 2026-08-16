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
