// Collection-tree task on the Android webview (specs/ui-work-loop.md platform
// matrix path 1: attach over CDP to the Tauri debug webview).
//
// The tree itself is an authed surface and the dev proxy strips Cookie
// headers (ui-work-loop Findings), so on-device coverage is the fixed
// matrix's anonymous half: the shell this task touched (bottom tabs, no
// tree fetch, no badge for anonymous) plus the newly vendored collapsible /
// item sections on the bench — the real-webview check the vendor-component
// checklist asks for.
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

const treeRequests = [];
page.on("request", (r) => {
  if (r.url().includes("collection_tree")) treeRequests.push(r.url());
});

// Navigate only with goto(): assigning location.href from an evaluate() tears
// down the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/catalog");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

// 1. Anonymous shell: both bottom tabs, and no My-cards badge.
const tabs = page.locator('nav[aria-label="Primary"] a');
if ((await tabs.count()) !== 2) fail(`expected 2 bottom tabs, got ${await tabs.count()}`);
else ok("bottom tabs render on the device");
const myTabBadge = page.locator(
  'nav[aria-label="Primary"] a[href="/my"] [data-name="Badge"]',
);
if ((await myTabBadge.count()) !== 0) fail("anonymous tab badge should be absent");
else ok("no My-cards badge for an anonymous session");

// 2. The anonymous shell must never fire the session tree read.
await page.waitForTimeout(1500);
if (treeRequests.length) fail(`anonymous shell fetched the tree: ${treeRequests[0]}`);
else ok("no collection_tree request from the anonymous shell");

// 3. Bench: collapsible collapses (data-state + inert) on the real webview.
await page.goto("http://tauri.localhost/dev/components");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
const content = page.locator("#bench-collapsible-open");
const trigger = page.locator('[aria-controls="bench-collapsible-open"]');
await trigger.scrollIntoViewIfNeeded();
if ((await content.getAttribute("data-state")) !== "open")
  fail("collapsible did not render open");
await trigger.click();
await page.waitForTimeout(400);
if ((await content.getAttribute("data-state")) !== "closed")
  fail("collapsible did not close on tap");
else ok("collapsible closes on tap");
const closedInert = await page
  .locator("#bench-collapsible-closed")
  .evaluate((el) => el.inert);
if (!closedInert) fail("closed collapsible content is not inert on this webview");
else ok("closed collapsible content is inert");
await trigger.click(); // restore

// 4. Bench: the item href arm rendered a real <a>.
if ((await page.locator('a[data-name="Item"]').count()) === 0)
  fail("item href arm rendered no <a> on the device");
else ok("item link row renders");

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

console.log(process.exitCode ? "ANDROID TREE CHECK FAIL" : "ANDROID TREE CHECK PASS");
await browser.close();
