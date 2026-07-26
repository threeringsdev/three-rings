// Android webview probe for the ⌘K palette's **desktop gate**
// (design/command-palette.md: "Not on mobile in v1").
//
// **This is a negative check, so it is built around a positive control.** The
// palette is absent from every Android page for two independent reasons — the
// webview is phone-width, and the dev proxy strips Cookie headers so the session
// never arrives (specs/ui-work-loop.md Findings) — and "the element isn't there"
// is also what a blank page looks like. So the probe asserts three things in
// order: an app page renders and is interactive while carrying no palette (and
// Ctrl+K does not summon one); the gate's own readout says `false` on the bench,
// which isolates the *viewport* half of the decision — that readout is the same
// `desktop_signal()` the app mounts on; and, separately, the vendored
// `CommandDialog` the palette is built from *does* open and rank on this webview
// when opened by hand. Without the last one, "no palette" would be
// indistinguishable from "the dialog is broken on Android".
//
// Prerequisites: the android-smoke skill's dev attach (emulator booted, app
// launched, `adb reverse tcp:3000 tcp:3000`, `adb forward tcp:9222 ...`).
// Navigate only with `page.goto` — a JS `location.href` races the CDP session.

import { chromium } from "@playwright/test";

const CDP = process.env.ANDROID_CDP ?? "http://127.0.0.1:9222";
const failures = [];

const browser = await chromium.connectOverCDP(CDP);
const ctx = browser.contexts()[0];
const page = ctx.pages()[0];

page.on("pageerror", (e) => failures.push(`pageerror: ${e.message}`));
page.on("console", (m) => {
  // `cargo leptos watch`'s live-reload socket lives on :3001, which the dev
  // attach does not `adb reverse` — its refusal is the harness talking.
  if (m.type() === "error" && !m.text().includes("live_reload")) {
    failures.push(`console error: ${m.text().slice(0, 200)}`);
  }
});

const hydrate = () =>
  page
    .locator("html[data-hydrated=true]")
    .waitFor({ state: "attached", timeout: 30000 });

// --- 1. an app page: the palette is not there, and the chord does not summon it.
await page.goto("http://tauri.localhost/catalog", { waitUntil: "load" });
await hydrate();
// Positive control: this really is the catalog, rendered and interactive.
if (!(await page.locator('nav[aria-label="Primary"] >> text=Catalog').isVisible())) {
  failures.push("the catalog page did not render its mobile tabs on the webview");
}
if ((await page.locator("#command-palette").count()) !== 0) {
  failures.push("the palette is mounted on an Android app page");
}
await page.keyboard.press("Control+k");
await page.waitForTimeout(300);
if ((await page.locator("#command-palette").count()) !== 0) {
  failures.push("Ctrl+K summoned the palette on the Android webview");
}

// --- 2. the bench, where the gate is observable on its own.
await page.goto("http://tauri.localhost/dev/components#command-dialog", { waitUntil: "load" });
await hydrate();

// --- positive control 1: the page is really here, on a really-small viewport.
const size = await page.evaluate(() => ({
  w: window.innerWidth,
  fine: window.matchMedia("(pointer: fine)").matches,
}));
if (!(await page.locator("#bench-palette-open").isVisible())) {
  failures.push("the palette bench section did not render on the webview");
}
if (size.w >= 768) {
  failures.push(
    `the emulator is not at phone width (${size.w}px) — this probe proves nothing there`,
  );
}

// --- the gate itself.
const readout = await page.textContent('[data-testid="bench-palette-desktop"]');
if (readout?.trim() !== "false") {
  failures.push(
    `the desktop gate reads ${JSON.stringify(readout)} on a ${size.w}px ` +
      `pointer-fine=${size.fine} webview; the palette would mount on mobile`,
  );
}

// --- positive control 2: the surface the gate is withholding does work here.
// Opened by hand (the chord listener lives in the app shell, not the bench).
await page.locator("#bench-palette-open").scrollIntoViewIfNeeded();
await page.locator("#bench-palette-open").click();
const dialog = page.locator("#command-palette[role=dialog]");
await dialog.waitFor({ state: "attached", timeout: 5000 });
if ((await dialog.getAttribute("data-state")) !== "open") {
  failures.push("CommandDialog did not open on the Android webview");
}
const rows = await page
  .locator("#command-palette [data-testid=palette-row]")
  .allTextContents();
if (rows.length < 4) {
  failures.push(`palette rows missing on the webview: ${JSON.stringify(rows)}`);
}
// Typing has to rank on-device too — the wireframe's P2 query.
await page.locator("#command-palette [data-name=CommandInput]").fill("tra");
await page.waitForTimeout(200);
const ranked = await page
  .locator("#command-palette [data-testid=palette-row]")
  .allTextContents();
if (!(ranked.length === 2 && /Trade Binder/.test(ranked[0]))) {
  failures.push(`ranking misbehaved on the webview: ${JSON.stringify(ranked)}`);
}

console.log(`=== ${page.url()} (${size.w}px, pointer-fine=${size.fine})`);
if (failures.length) {
  console.log(failures.map((f) => `  ✗ ${f}`).join("\n"));
  process.exitCode = 1;
} else {
  console.log(
    "CLEAN — the ⌘K desktop gate reads false at phone width, and CommandDialog itself works on the Android webview",
  );
}
await browser.close();
