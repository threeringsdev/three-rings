// Collection (binder/deck) view on the Android webview (specs/ui-work-loop.md
// platform matrix path 1: attach over CDP to the Tauri debug webview).
//
// **What this task can and cannot cover on-device.** `/my/collections/:id` is
// authed and the dev proxy strips Cookie headers (ui-work-loop Findings), so
// the table itself is unreachable here — the same fixed matrix `/my` and the
// collection tree ran under. What *is* reachable is the half that is anonymous
// or lives on the bench, and it is the platform-risky half:
//
//   1. the new route bouncing to `/login?next=/my/collections/<id>`, which on
//      this platform goes through the `data-ssr-path` shim rather than a
//      browser-followed 302 (the in-process proxy swallows redirects — the trap
//      that hydration-panicked every page during the shell task). A route the
//      router does not know would 404 into the fallback instead;
//   2. the **count stepper**, which this task places in the HERE column: it is
//      the one piece of input the page adds, and touch is where it is most
//      fragile (WebKit/Android don't focus a tapped `<button>`, so the whole
//      blur-commit anchoring is engine-dependent). `android-stepper-check.mjs`
//      drives its behavior; here we pin the *table* arrangement it now lives
//      in — a stepper inside a row that reveals on `group/row`, sized so a
//      phone-width row does not scroll the page sideways;
//   3. the vendored `breadcrumb`, which this task is the first consumer of.
//
// Prereqs (android-smoke skill): the debug app running on the emulator with
// `adb reverse tcp:3000 tcp:3000`, and
//   pid=$(adb shell ps -A | grep three_rings | awk '{print $2}' | head -1)
//   adb forward tcp:9222 "localabstract:webview_devtools_remote_$pid"
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

const hydrated = () =>
  page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

// A plausible-looking id: the assertion is about the *route*, not the row.
const ID = "00000000-0000-4000-8000-000000000000";

// Navigate only with goto(): assigning location.href from an evaluate() tears
// down the execution context mid-call (ui-work-loop Findings).
await page.goto(`http://tauri.localhost/my/collections/${ID}`);
await hydrated();

// 1. The guard bounce, through the redirect-swallowing shim. Getting the login
//    page here means the router matched the new route, ran the `/my/*` guard,
//    and the shim recovered the URL — three things that only line up on this
//    platform if all of them work.
const url = page.url();
if (!url.includes("/login")) fail(`expected the login bounce, got ${url}`);
else if (!url.includes(`next=/my/collections/${ID}`))
  fail(`login bounce lost the return path: ${url}`);
else ok("anonymous collection URL bounces to /login carrying its return path");

if ((await page.locator("input[name=email]").count()) !== 1)
  fail("the bounce did not land on a real login form");
else ok("the shim recovered a hydrated login page (no 404 fallback)");

// 2. The stepper, in the row arrangement this task ships. The bench's basic
//    stepper is the same component the HERE column mounts; on a touch device
//    the reveal classes must not hide it from a tap.
await page.goto("http://tauri.localhost/dev/components");
await hydrated();
const stepper = page.locator("#bench-stepper-basic [data-testid=count-stepper]");
await stepper.scrollIntoViewIfNeeded();
const inc = stepper.locator("[data-testid=count-stepper-inc]");
const box = await inc.boundingBox();
if (!box || box.width < 20 || box.height < 20)
  fail(`stepper + control is not tappable on this device: ${JSON.stringify(box)}`);
else ok(`stepper + control is tappable (${Math.round(box.width)}×${Math.round(box.height)})`);

const before = await stepper
  .locator("[data-testid=count-stepper-value]")
  .textContent();
// `.click()` rather than `.tap()`: the CDP-attached context has no
// `hasTouch` option to set, and the webview delivers the click as a real
// pointer sequence on the device either way (the stepper probe does the same).
await inc.click();
await page.locator("h1").first().click(); // blur out of the stepper
await page.waitForTimeout(400);
const after = await stepper
  .locator("[data-testid=count-stepper-value]")
  .textContent();
if (Number(after) !== Number(before) + 1)
  fail(`tap+blur did not commit on-device: ${before} → ${after}`);
else ok(`tap + blur committed on the real webview (${before} → ${after})`);
// Put the bench back where it was, so a re-run starts from the same count.
const toast = page.locator('[data-name="Toast"]').first();
await toast.getByRole("button", { name: "Undo" }).click();
await page.waitForTimeout(300);

// 3. The breadcrumb primitive this task is the first consumer of renders here.
//    (Its page is authed, so the bench's own section is what is reachable.)
if ((await page.locator('[data-name="Breadcrumb"]').count()) === 0)
  fail("no breadcrumb rendered on the bench");
else ok("vendored breadcrumb renders on the device");

// 4. Phone-width layout: nothing may scroll the document sideways.
const overflow = await page.evaluate(
  () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
);
if (overflow > 1) fail(`document overflows horizontally by ${overflow}px`);
else ok("no horizontal overflow at phone width");

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

console.log(
  process.exitCode ? "ANDROID COLLECTION CHECK FAIL" : "ANDROID COLLECTION CHECK PASS",
);
await browser.close();
