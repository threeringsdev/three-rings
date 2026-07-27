// Selection tray on the Android webview (ui-work-loop platform matrix path 1:
// attach over CDP to the Tauri debug webview).
//
// The tray is layout code — an overlapping thumbnail stack sized in absolute
// pixels, and a `role=checkbox` button that has to toggle on the WebView
// engine — so the matrix asks for an on-device pass. Both surfaces that host
// row checkboxes (`/my`,
// `/my/collections/:id`) are authed and the dev proxy strips Cookie headers
// (app-ui Findings), so on-device coverage is the bench section, which drives
// the same `SelectionCheckbox` + `SelectionTray` the pages host.
//
// What is deliberately NOT covered here: the fixed dock above the mobile tab
// bar lives in the shell, which only renders on the authed pages. That is
// asserted in the chromium tier at 390×844 instead.
//
// Input note: a CDP-attached context has no `hasTouch`, so `locator.tap()`
// throws ("The page does not support tap") and every gesture here is `click()`
// — the same limitation the other android-*-check probes work under. What the
// device still proves is the real WebView engine and its layout.
//
// Prereqs (android-smoke skill): `cargo tauri android dev` running, and
//   socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | tail -1)
//   adb forward tcp:9222 "localabstract:$socket"
// (`head -1` picks a stale socket when an earlier debug process is still
// listed; `tail -1` was the live one here.)
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

const rows = page.locator('#bench-tray-rows [data-testid="row-select"]');
const tray = page.locator('[data-testid="selection-tray"]');
const count = page.locator('[data-testid="tray-count"]');
const thumbs = page.locator('[data-testid="tray-thumb"]');

await rows.first().scrollIntoViewIfNeeded();

if ((await rows.count()) === 4) ok("bench renders four selectable rows");
else fail(`expected 4 bench rows, got ${await rows.count()}`);

if ((await tray.count()) === 0) ok("no tray at zero selection");
else fail("tray rendered with an empty selection");

// The control is a <button role=checkbox>, not a native <input>, so this is
// the path that could differ from desktop on the WebView engine.
await rows.nth(0).click();
await page.waitForTimeout(150);
if ((await tray.count()) === 1 && (await count.textContent()) === "1 card")
  ok("first pick raises the tray reading “1 card”");
else
  fail(
    `first pick did not raise a 1-card tray (count=${await tray.count()}, text=${await count.textContent()})`,
  );
if ((await rows.nth(0).getAttribute("aria-checked")) === "true")
  ok("the picked row announces aria-checked=true");
else fail("picked row did not announce checked");

// Four picks, three thumbnails: the stack caps, the count does not.
for (const i of [1, 2, 3]) await rows.nth(i).click();
await page.waitForTimeout(150);
if ((await count.textContent()) === "4 cards") ok("count reads “4 cards”");
else fail(`count did not reach 4 cards (got ${await count.textContent()})`);
if ((await thumbs.count()) === 3) ok("thumbnail stack caps at three");
else fail(`expected 3 thumbnails, got ${await thumbs.count()}`);

// The thumbnail that has art renders a real <img> laid out at the wireframe's
// 22×30 — the box a broken data URI or a collapsed flex child would lose.
const box = await thumbs.first().boundingBox();
if (box && Math.round(box.width) === 22 && Math.round(box.height) === 30)
  ok("thumbnail lays out at 22×30");
else fail(`thumbnail box wrong: ${JSON.stringify(box)}`);

// "Move to…" opens the destination picker — a `popover` positioned with CSS
// anchor positioning, which is exactly the vendoring checklist's
// native-webview item. What is proved here is that the panel opens and lands
// **on screen** on the real WebView engine; what it lists is a session read the
// dev proxy cannot make (it strips Cookie headers), so what renders is the
// **failed** arm.
//
// This used to expect "No collection to move to." and call that "the honest
// rendering". It was not: the read 401s, and an empty list is a claim about the
// account rather than about the read — the same correction made in
// tests/selection-tray.spec.ts. `DestinationList` now separates the two, and the
// empty line is reserved for what it can actually speak about: a *filter* that
// matched nothing.
const move = page.locator('[data-testid="tray-move"]');
if (!(await move.isDisabled())) ok("“Move to…” is live on-device");
else fail("“Move to…” is still disabled on-device");
await move.click();
await page.waitForTimeout(400);
const picker = page.locator("#popover-tray-destination");
if (await picker.evaluate((el) => el.matches(":popover-open")))
  ok("the picker opens on the WebView engine");
else fail("the picker did not open on-device");
if ((await picker.locator('[data-name="CommandInput"]').count()) === 1)
  ok("the picker brings the catalog control's search box with it");
else fail("no CommandInput inside the on-device picker");
{
  const text = (await picker.textContent()) ?? "";
  if (!text.includes("Couldn't load your collections.")) {
    fail(`unexpected picker content: ${text}`);
  } else if (text.includes("No collection to move to.")) {
    fail("the picker still claims the account has no collections");
  } else {
    ok("anonymous on-device picker blames the read, not the account");
  }
}
const panel = await picker.boundingBox();
const viewport = page.viewportSize() ?? (await page.evaluate(() => ({
  width: window.innerWidth,
  height: window.innerHeight,
})));
if (
  panel &&
  panel.x >= 0 &&
  panel.y >= 0 &&
  panel.x + panel.width <= viewport.width + 1 &&
  panel.height > 0
)
  ok("the picker lands on screen (anchor positioning / JS fallback)");
else
  fail(
    `picker off screen: ${JSON.stringify(panel)} vs ${JSON.stringify(viewport)}`,
  );
await page.keyboard.press("Escape");
await page.waitForTimeout(200);
if ((await count.textContent()) === "4 cards")
  ok("opening and dismissing the picker left the selection alone");
else fail("the picker changed the selection");

// Clear empties it and un-checks every row.
await page.locator('[data-testid="tray-clear"]').click();
await page.waitForTimeout(150);
if ((await tray.count()) === 0) ok("clear removes the tray entirely");
else fail("tray survived clear");
const checked = await page.$$eval(
  '#bench-tray-rows [data-testid="row-select"]',
  (els) => els.filter((e) => e.getAttribute("aria-checked") === "true").length,
);
if (checked === 0) ok("clear un-checked every row");
else fail(`${checked} rows still checked after clear`);

if (pageErrors.length === 0) ok("no page errors");
else fail(`page errors: ${pageErrors.join(" | ")}`);

await browser.close();
console.log(
  process.exitCode ? "ANDROID SELECTION-TRAY CHECK FAIL" : "ANDROID SELECTION-TRAY CHECK PASS",
);
