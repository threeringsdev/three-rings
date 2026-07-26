// Catalog paging (`?cursor=`) on the Android webview — one-off task probe
// (android-smoke skill, matrix path 1: attach over CDP to the Tauri debug
// webview). `/catalog` is public, so unlike `/my` the whole feature is
// reachable here despite the dev proxy stripping Cookie headers.
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

// The page's own data, through the same in-process proxy the app uses.
const api = async (qs) =>
  JSON.parse(
    await page.evaluate(
      async (u) => await (await fetch(u)).text(),
      `/api/catalog/search?${qs}`,
    ),
  );

// Navigate only with goto(): assigning location.href from an evaluate() tears
// down the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/catalog");
await hydrated();

const first = await api("q=");
if (!first.next_cursor) fail("browse-all has no second page to walk to");
const second = await api(`q=&cursor=${first.next_cursor}`);

// 1. The pager renders on the device.
if (!(await page.locator('[data-testid="page-next"]').count()))
  fail("no Next control on browse-all page one");
else ok("pager renders on the webview");
if (await page.locator('[data-testid="page-first"]').count())
  fail("page one offered Back to the start");
else ok("page one offers no Back to the start");

// 2. Clicking it pages forward. `click()`, not `tap()`: Playwright's touch
// emulation needs `hasTouch` on the *context*, and an attached CDP context
// cannot be reconfigured from here (android-rail-check.mjs found this first).
// The click still goes through the device's real webview.
const nextTarget = await page
  .locator('[data-testid="page-next"]')
  .boundingBox();
if (!nextTarget || nextTarget.height < 24)
  fail(`Next is not a tappable size: ${JSON.stringify(nextTarget)}`);
else ok(`Next has a tappable height (${Math.round(nextTarget.height)}px)`);
await page.locator('[data-testid="page-next"]').click();
await page
  .waitForURL(new RegExp(`cursor=${first.next_cursor}`), { timeout: 8000 })
  .then(
    () => ok("tapping Next put the cursor in the URL"),
    () => fail(`tap did not page — at ${page.url()}`),
  );
// Waited for, not read once: `Results` is a `<Transition>`, so page one stays
// on screen until page two resolves — reading the grid the instant the URL
// moves catches the *old* page every time (it did, first run).
const tile = (oracleId) =>
  page.locator(`[data-testid=results-grid] a[href="/cards/${oracleId}"]`);
await tile(second.cards[0].oracle_id)
  .waitFor({ state: "attached", timeout: 10000 })
  .then(
    () => ok("page two shows the rows that follow page one"),
    () => fail("page two never rendered the row after the cursor"),
  );
if (await tile(first.cards[0].oracle_id).count())
  fail("page one's first card is still on screen");
else ok("page one's rows are gone");

// 3. A query edit drops the cursor — the rule the whole feature rests on,
//    re-checked on the engine that is not chromium-desktop.
await page.locator("#catalog-query").fill("bolt");
await page.waitForURL(/\/catalog\?q=bolt$/, { timeout: 8000 }).then(
  () => ok("typing dropped the cursor (URL is page one of the new query)"),
  () => fail(`cursor survived a query edit — at ${page.url()}`),
);

// 4. A deep-linked cursor SSRs on the device (the dev proxy path, not a
//    browser-followed request).
const bolt = await api("q=bolt&limit=1");
await page.goto(
  `http://tauri.localhost/catalog?q=bolt&cursor=${bolt.next_cursor}`,
);
await hydrated();
if (!(await page.locator('[data-testid="page-first"]').count()))
  fail("a deep-linked cursored page offered no way home");
else ok("deep-linked cursored page renders with Back to the start");
if (await page.locator('[data-testid="page-next"]').count())
  fail("the last page offered a Next");
else ok("the last page offers no Next");

// 5. No horizontal overflow at phone width from the pager's two-link row.
const overflow = await page.evaluate(() => {
  const d = document.documentElement;
  return d.scrollWidth - d.clientWidth;
});
if (overflow > 0) fail(`horizontal overflow of ${overflow}px`);
else ok("no horizontal overflow at phone width");

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

await browser.close();
console.log(
  process.exitCode
    ? "ANDROID CATALOG PAGING: FAIL"
    : "ANDROID CATALOG PAGING: PASS",
);
