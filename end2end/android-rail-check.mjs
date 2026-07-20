// Filter rail on the Android webview (specs/ui-work-loop.md platform matrix
// path 1: attach over CDP to the Tauri debug webview).
//
// Not a Playwright test — the suite runs against :3000 in a desktop browser.
// This drives the *device* webview, which is where the mobile surface actually
// lives: the slide-over sheet, its badge, and the touch targets that the
// desktop viewport never exercises.
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

// Navigate only with goto(): assigning location.href from an evaluate() tears
// down the execution context mid-call (ui-work-loop Findings).
const go = async (q) => {
  await page.goto(`http://tauri.localhost/catalog?q=${encodeURIComponent(q)}`);
  await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
};

// 1. The mobile surface is the sheet, not the rail — the emulator is a phone.
await go("t:instant c:ur cmc<=2");
const badge = page.getByTestId("filter-badge");
if ((await badge.count()) === 0) fail("no active-filter badge on the device");
else if ((await badge.innerText()).trim() !== "4")
  fail(`badge should count 4 active filters, got ${await badge.innerText()}`);
else ok("active-filter badge counts the URL's filters");

if (await page.locator("[data-testid=filter-rail]").isVisible())
  fail("the desktop rail should be hidden at phone width");
else ok("desktop rail hidden at phone width");

// 2. The sheet opens and reflects the query into its widgets.
//
// `click()`, not `tap()`: Playwright's touch emulation needs `hasTouch` on the
// context, and a CDP-attached context is the app's own — its options cannot be
// set from here. The click still goes through the device's real Chrome 145
// engine, which is what this check exists to exercise.
await page.getByRole("button", { name: /Filters/ }).click();
const sheet = page.locator("[data-testid=filter-sheet]");
await sheet.waitFor({ state: "visible", timeout: 5000 });
ok("filter sheet opens on click");

const instant = sheet.getByRole("checkbox", { name: "Instant" });
if ((await instant.getAttribute("aria-checked")) !== "true")
  fail("sheet did not reflect t:instant into its Type facet");
else ok("sheet reflects the query into its widgets");

// 3. A click inside the sheet rewrites the query — the whole point of the
//    two-surface design, exercised on the device's real engine.
await sheet.getByRole("checkbox", { name: "Sorcery" }).click();
await page.waitForURL(
  (url) =>
    (url.searchParams.get("q") ?? "").includes("t:instant,sorcery"),
  { timeout: 5000 },
);
ok("a facet click rewrites the query text");

if ((await badge.innerText()).trim() !== "5")
  fail(`badge should follow the edit to 5, got ${await badge.innerText()}`);
else ok("badge follows the edit");

// 4. Scroll lock: the sheet is an overlay, so the page behind it must not
//    scroll (the vendored use_scroll_lock behavior).
const overflow = await page.evaluate(() => document.body.style.overflow);
if (overflow !== "hidden")
  fail(`body should be scroll-locked while the sheet is open, got "${overflow}"`);
else ok("body scroll-locked while the sheet is open");

console.log(
  process.exitCode ? "ANDROID RAIL CHECK FAIL" : "ANDROID RAIL CHECK PASS",
);
await browser.close();
