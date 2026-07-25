// Android webview probe for the quick-add panel's keystroke machinery
// (specs/app-ui.md → Quick-add panel).
//
// **Why the bench and not the panel.** The panel lives on `/my/collections/:id`,
// which is authed, and the Tauri Android *dev* proxy strips Cookie headers and
// POST bodies (specs/ui-work-loop.md Findings) — so neither the page nor the add
// itself is reachable from the emulator until the queued "Android release auth
// check" task lands. What *is* reachable is the mechanism the panel is built on:
// `use_command_nav`, a feature-owned input driving `command`'s item registry with
// its own modifier handling, demoed anonymously at `/dev/components`. That is the
// half that could plausibly behave differently on a webview (key events, focus,
// `aria-selected` reflection), so it is the half worth checking on-device.
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
  // attach does not `adb reverse` — its refusal is the harness talking, not the
  // page. Everything else counts.
  if (m.type() === "error" && !m.text().includes("live_reload")) {
    failures.push(`console error: ${m.text().slice(0, 200)}`);
  }
});

await page.goto("http://tauri.localhost/dev/components", { waitUntil: "load" });
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached", timeout: 30000 });

const input = page.locator("#bench-command-foreign");
await input.scrollIntoViewIfNeeded();
await input.click();

const state = () => page.textContent('[data-testid="bench-command-foreign-state"]');

// ↑↓ move the highlight through the registry from an input the primitive does
// not own — the panel's navigation, minus the panel.
await input.press("ArrowDown");
await input.press("ArrowDown");
await page.waitForTimeout(200);
if (!(await state()).includes("row 2")) {
  failures.push(`↓↓ did not reach row 2 on the webview: ${await state()}`);
}
await input.press("ArrowUp");
await page.waitForTimeout(150);
if (!(await state()).includes("row 1")) {
  failures.push(`↑ did not move back on the webview: ${await state()}`);
}

// The primitive's own `aria-selected` has to agree with the index the caller
// renders its chip from, or the panel would highlight one row and add another.
const selected = await page
  .locator('#bench-command-nav [data-name="CommandItem"][aria-selected="true"]')
  .allTextContents();
if (!(selected.length === 1 && /Trade Binder/.test(selected[0]))) {
  failures.push(`aria-selected disagrees with the highlight: ${JSON.stringify(selected)}`);
}

// ⌥⏎ — the modifier `CommandInput`'s own handler never sees, and the shape the
// panel's "want instead" is built on. Android reports Alt as a real modifier.
await input.press("Alt+Enter");
await page.waitForTimeout(250);
if (!(await state()).includes("picked: TRADE BINDER")) {
  failures.push(`⌥⏎ did not carry the modifier through on the webview: ${await state()}`);
}

// Plain ⏎ still activates without the modifier.
await input.press("ArrowUp");
await input.press("Enter");
await page.waitForTimeout(250);
if (!(await state()).includes("picked: Inbox")) {
  failures.push(`⏎ did not activate the highlighted row: ${await state()}`);
}

console.log(`=== ${page.url()}`);
if (failures.length) {
  console.log(failures.map((f) => `  ✗ ${f}`).join("\n"));
  process.exitCode = 1;
} else {
  console.log("CLEAN — foreign-input ↑↓/⏎/⌥⏎ over command's registry OK on the Android webview");
}
await browser.close();
