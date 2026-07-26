// The catalog's "N owned" badge on the Android webview — one-off task probe
// (android-smoke skill, matrix path 1: attach over CDP to the Tauri debug
// webview).
//
// This covers the **anonymous** half, deliberately: the dev proxy strips Cookie
// headers, so the webview can only ever be an unauthenticated caller here — and
// that is the half worth watching on-device anyway, because the badge must NOT
// appear and `owned` must arrive as `null` (unknown) rather than 0. The authed
// half is the chromium tier's (`catalog.spec.ts`).
//
// Prereqs: `cargo tauri android dev` running, and
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
await page.goto("http://tauri.localhost/catalog?q=t%3Acreature");
await hydrated();

// Positive control first — "no badges" is also true of a page with no results.
const tiles = await page.locator("[data-testid=results-grid] li").count();
if (tiles > 0) ok(`grid rendered ${tiles} tiles`);
else fail("no result tiles — the badge assertions below would be vacuous");

const badges = await page.getByTestId("owned-badge").count();
if (badges === 0) ok("no owned badge for an unauthenticated webview");
else fail(`${badges} owned badge(s) rendered without a session`);

// The wire value, through the same in-process proxy the app uses: `null`, not
// 0. A projection that defaulted the column would render the identical
// badge-free page above, so only this distinguishes unknown from "holds none".
const results = JSON.parse(
  await page.evaluate(
    async (u) => await (await fetch(u)).text(),
    "/api/catalog/search?q=t%3Acreature&limit=10",
  ),
);
const filled = results.cards.filter((c) => c.owned !== null);
if (results.cards.length === 0) fail("search returned no cards");
else if (filled.length === 0) ok(`owned is null on all ${results.cards.length} hits`);
else fail(`owned filled on ${filled.length} hit(s) without a session`);

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

console.log(process.exitCode ? "ANDROID OWNED-BADGE FAIL" : "ANDROID OWNED-BADGE PASS");
await browser.close();
