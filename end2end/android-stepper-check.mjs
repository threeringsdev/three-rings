// Count-stepper on the Android webview (ui-work-loop platform matrix path 1:
// attach over CDP to the Tauri debug webview). The stepper is layout/input
// code — hover-reveal, click-to-type field swap, focus/blur commit lifecycle
// — so the platform matrix requires an on-device pass. The collection-view
// surface that hosts it is authed and the dev proxy strips Cookie headers
// (ui-work-loop Findings), so on-device coverage is the bench section, which
// exercises the same component end to end.
//
// Touch note: WebKit/Chromium on Android does not focus a <button> on tap the
// way a mouse click does on desktop; the stepper anchors focus programmatically
// so blur-commit works, and this probe drives the real engine to confirm it.
//
// Prereqs (android-smoke skill): `cargo tauri android dev` running, and
//   socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | head -1)
//   adb forward tcp:9222 "localabstract:$socket"
import { chromium } from "playwright";

const fail = (msg) => {
  console.error(`FAIL: ${msg}`);
  process.exitCode = 1;
};
const ok = (msg) => console.log(`  ok  ${msg}`);

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const page = browser.contexts()[0].pages()[0];
const pageErrors = [];
page.on("pageerror", (e) => pageErrors.push(String(e).slice(0, 200)));

// Navigate only with goto(): assigning location.href from evaluate() tears
// down the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/dev/components");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

const value = page.locator('#bench-stepper-basic [data-testid="count-stepper-value"]');
const inc = page.locator('#bench-stepper-basic [data-testid="count-stepper-inc"]');
const last = page.locator('[data-testid="bench-stepper-last"]');

await value.scrollIntoViewIfNeeded();

// Tap + twice, then blur → one commit 3 → 5 (pending accumulates, commits once).
await inc.click();
await inc.click();
await page.waitForTimeout(150);
if ((await value.textContent())?.trim() === "5") ok("tap + accumulates pending to 5");
else fail(`tap ++ did not show pending 5 (got ${await value.textContent()})`);

await page.evaluate(() => document.activeElement?.blur());
await page.waitForTimeout(250);
if ((await last.textContent()) === "3 → 5") ok("blur committed once (3 → 5)");
else fail(`blur did not commit 3 → 5 (got ${await last.textContent()})`);

// Undo toast is reachable and reverses on the touch engine.
const undo = page.locator('[data-name="Toast"] button', { hasText: "Undo" });
if ((await undo.count()) > 0) {
  await undo.click();
  await page.waitForTimeout(200);
  if ((await value.textContent())?.trim() === "3") ok("undo restored the count on-device");
  else fail(`undo did not restore (got ${await value.textContent()})`);
} else {
  fail("no undo toast raised on-device");
}

// Click-to-type: tapping the count mounts the vendored Input, seeded from the
// bound signal (the SSR value-attribute path), and it commits the typed value.
await value.click();
await page.waitForTimeout(200);
const input = page.locator('#bench-stepper-basic [data-testid="count-stepper-input"]');
if ((await input.count()) > 0) {
  ok("tap-to-type mounted the Input");
  if ((await input.inputValue()) === "3") ok("Input seeded with the count");
  else fail(`Input not seeded (got ${await input.inputValue()})`);
  await input.fill("8");
  await input.press("Enter");
  await page.waitForTimeout(200);
  if ((await value.textContent())?.trim() === "8") ok("Enter committed the typed count");
  else fail(`Enter did not commit typed count (got ${await value.textContent()})`);
} else {
  fail("tap-to-type mounted no Input");
}

if (pageErrors.length) fail(`pageerrors: ${pageErrors.join(" | ")}`);
await browser.close();
console.log(process.exitCode ? "ANDROID STEPPER CHECK FAIL" : "ANDROID STEPPER CHECK PASS");
