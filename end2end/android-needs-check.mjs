// Needs + shopping on the Android webview (specs/ui-work-loop.md platform
// matrix path 1: attach over CDP to the Tauri debug webview).
//
// **What this task can and cannot cover on-device.** Both new routes are authed
// and the dev proxy strips Cookie headers (ui-work-loop Findings), so neither
// table is reachable here — the same fixed matrix `/my`, the tree and the
// collection view ran under. What *is* reachable is the platform-risky half:
//
//   1. **two new routes** bouncing to `/login?next=…`, which on this platform
//      goes through the `data-ssr-path` shim rather than a browser-followed 302
//      (the in-process proxy swallows redirects). A route the router does not
//      know would land in the 404 fallback instead — which is exactly what these
//      two URLs did until this task, since they pointed at placeholder views.
//      The nested `/needs` segment is the interesting one: it is the app's first
//      three-segment route, and a mis-declared `ParamSegment` chain would match
//      the two-segment collection route (or nothing) rather than this one;
//   2. **the checkbox in a list row**, which the pick list is built on. Its
//      behavior is Leptos-owned (`role=checkbox` on a `<button>`, not a native
//      input), so a tap that does not toggle would silently make every pick-list
//      line un-tickable on a phone — the one control this task adds that a
//      desktop run cannot vouch for.
//
// Prereqs (android-smoke skill): the debug app running on the emulator with
// `adb reverse tcp:3000 tcp:3000`, and
//   socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | tail -1)
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

const hydrated = () =>
  page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

// A plausible-looking id: the assertion is about the *route*, not the rows.
const ID = "00000000-0000-4000-8000-000000000000";

// 1. Both new routes, through the redirect-swallowing shim.
for (const [path, label] of [
  [`/my/collections/${ID}/needs`, "needs"],
  ["/my/shopping", "shopping"],
]) {
  // Navigate only with goto(): assigning location.href from an evaluate()
  // tears down the execution context mid-call (ui-work-loop Findings).
  await page.goto(`http://tauri.localhost${path}`);
  await hydrated();
  const url = page.url();
  if (!url.includes("/login")) fail(`${label}: expected the login bounce, got ${url}`);
  else if (!url.includes(`next=${path}`))
    fail(`${label}: login bounce lost the return path: ${url}`);
  else ok(`anonymous ${label} URL bounces to /login carrying its return path`);

  if ((await page.locator("input[name=email]").count()) !== 1)
    fail(`${label}: the bounce did not land on a real login form`);
  else ok(`${label}: the shim recovered a hydrated login page (no 404 fallback)`);
}

// 2. The checkbox the pick list ticks with. Its own page is authed, so the
//    bench section is what is reachable — same component, same wiring.
await page.goto("http://tauri.localhost/dev/components");
await hydrated();
const box = page.locator('[data-name="Checkbox"]').first();
await box.scrollIntoViewIfNeeded();
const rect = await box.boundingBox();
if (!rect || rect.width < 14 || rect.height < 14)
  fail(`checkbox is not tappable on this device: ${JSON.stringify(rect)}`);
else ok(`checkbox is tappable (${Math.round(rect.width)}×${Math.round(rect.height)})`);

const before = await box.getAttribute("aria-checked");
// `.click()` rather than `.tap()`: the CDP-attached context has no `hasTouch`
// option to set, and the webview delivers the click as a real pointer sequence
// either way (the stepper probe does the same).
await box.click();
await page.waitForTimeout(300);
const after = await box.getAttribute("aria-checked");
if (after === before)
  fail(`tap did not toggle the checkbox on-device (stayed ${before})`);
else ok(`tap toggles the checkbox on the real webview (${before} → ${after})`);
// Leave the bench as it was found.
await box.click();
await page.waitForTimeout(200);

// 3. Phone-width layout: nothing may scroll the document sideways.
const overflow = await page.evaluate(
  () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
);
if (overflow > 1) fail(`document overflows horizontally by ${overflow}px`);
else ok("no horizontal overflow at phone width");

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

console.log(process.exitCode ? "ANDROID NEEDS CHECK FAIL" : "ANDROID NEEDS CHECK PASS");
await browser.close();
