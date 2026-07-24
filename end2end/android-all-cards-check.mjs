// All-cards task on the Android webview (specs/ui-work-loop.md platform matrix
// path 1: attach over CDP to the Tauri debug webview).
//
// **What this task can and cannot cover on-device.** `/my` is authed and the
// dev proxy strips Cookie headers (ui-work-loop Findings), so the table itself
// is unreachable here. What *is* reachable is the half of this task that is
// anonymous, and it is the risky half anyway:
//
//   1. the shared `QueryBar` (app/src/components/query_bar.rs) — extracted from
//      `/catalog` by this task so `/my` could reuse it. Typing in it on the real
//      webview exercises the debounce, the router navigation, and the URL⇄field
//      sync on the engine that is not chromium-desktop;
//   2. `/my` anonymously bouncing to `/login?next=/my`, which on this platform
//      goes through the `data-ssr-path` shim rather than a browser-followed 302
//      (the in-process proxy swallows redirects — the trap that hydration-
//      panicked every page during the shell task).
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

const hydrated = () =>
  page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

// Navigate only with goto(): assigning location.href from an evaluate() tears
// down the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/catalog");
await hydrated();

// 1. The shared query bar is on the page and seeded from the URL.
const field = page.locator("#catalog-query");
if ((await field.count()) !== 1) fail("shared query bar missing from /catalog");
else ok("shared QueryBar renders on the device");

// 2. Typing moves the URL after the debounce — the whole point of the bar.
await field.fill("bolt");
await page.waitForURL(/\/catalog\?q=bolt/, { timeout: 8000 }).then(
  () => ok("typing navigated the URL (debounce fired on the webview)"),
  () => fail(`typing did not move the URL — at ${page.url()}`),
);
if (!(await page.locator('[data-testid="result-count"]').count()))
  fail("results toolbar missing after a search");
else ok("results rendered for the typed query");

// 3. …and the field still holds what was typed (the self_pushed guard: the
//    URL must not win an argument it started).
if ((await field.inputValue()) !== "bolt")
  fail(`field was clobbered by the navigation: "${await field.inputValue()}"`);
else ok("field kept its text across its own navigation");

// 4. Clear empties both the field and the URL.
await page.locator('button[aria-label="Clear search"]').click();
await page.waitForURL(/\/catalog$/, { timeout: 8000 }).then(
  () => ok("clear reset the URL"),
  () => fail(`clear did not reset the URL — at ${page.url()}`),
);
if ((await field.inputValue()) !== "")
  fail(`field not cleared: "${await field.inputValue()}"`);
else ok("clear emptied the field");

// 5. Back/Forward re-seeds the field from the URL (the other half of the sync).
await page.goBack();
await page.waitForTimeout(500);
if (!/q=bolt/.test(page.url()))
  fail(`back did not restore the query URL: ${page.url()}`);
else if ((await field.inputValue()) !== "bolt")
  fail(`back did not re-seed the field: "${await field.inputValue()}"`);
else ok("Back re-seeded the field from the URL");

// 6. `/my` anonymously lands on /login carrying the return path. On this
//    platform the proxy follows the 302 in-process, so this is the
//    data-ssr-path shim's path, not the browser's.
await page.goto("http://tauri.localhost/my");
await page.waitForTimeout(1500);
const url = new URL(page.url());
if (url.pathname !== "/login" || url.searchParams.get("next") !== "/my")
  fail(`anonymous /my did not bounce to /login?next=/my — at ${page.url()}`);
else ok("anonymous /my bounced to /login?next=/my");
await hydrated();
if (!(await page.locator('input[name="email"]').count()))
  fail("login form did not render after the bounce");
else ok("login page hydrated after the bounce");

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

await browser.close();
console.log(
  process.exitCode ? "\nANDROID CHECK FAILED" : "\nANDROID CHECK PASSED",
);
