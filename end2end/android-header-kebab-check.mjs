// The collection-header kebab on the **real Android webview** (ui-work-loop
// platform matrix path 1: attach over CDP to the Tauri debug webview). This is
// the touch half of the `Header Kebab` / `M Header Kebab` task.
//
// Why the bench and not `/my/collections/:id`: that surface is authed and the
// Tauri dev proxy strips Cookie headers, so on-device it redirects to
// `/login?next=…` (re-measured for this task — `android-cdp-check` lands on
// exactly that URL). The kebab exists *because* a phone has no other way into
// tree management once the tree is behind a drawer, so "a real touch on this
// button opens the menu" is the one claim that most needs a real engine, and the
// bench is the only anonymous place the button lives.
//
// What is asserted, all by real touch:
//
//   1. the kebab is a **44 px** tap target at phone width — the frame's bare
//      18 px glyph is the look, not the hit area, and this is the only place the
//      claim is checked on the engine that will actually be tapped;
//   2. a **tap** opens the shared panel through `ContextMenuHandle::open_at`,
//      having **aimed** it first (a panel that opened before its subject was set
//      would render the previous one on its first pass);
//   3. the panel is clamped inside the phone viewport — a menu hanging off the
//      bottom of a phone is a menu you cannot use, and the header kebab sits
//      much lower in the page than the tree rows do;
//   4. a **tap** on an item runs its action and closes the menu — the step the
//      whole touch path ends on;
//   5. the rail drawer is shut throughout, so none of the above is secretly
//      relying on the tree being on screen.
//
// Prereqs (android-smoke skill): the debug app installed and the repo-root
// `cargo leptos watch` serving :3000 via `adb reverse tcp:3000 tcp:3000`, then
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

const cdp = await page.context().newCDPSession(page);

/** A real touch. `Input.dispatchTouchEvent` is the only way to press rather than
 *  click; the point carries an `id` and a radius because without them Chrome's
 *  gesture pipeline does not track the contact across events. */
async function touch(x, y, holdMs) {
  const pt = { x, y, id: 1, radiusX: 8, radiusY: 8, force: 1 };
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [pt],
  });
  if (holdMs) await page.waitForTimeout(holdMs);
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
  });
}

/** Poll a predicate — the webview synthesizes a click from a touch on its own
 *  schedule, and a fixed sleep either flakes or hides how long it took. */
async function until(pred, ms = 3000) {
  const deadline = Date.now() + ms;
  for (;;) {
    if (await pred()) return true;
    if (Date.now() > deadline) return false;
    await page.waitForTimeout(100);
  }
}

async function tapCenter(selector) {
  const box = await page.locator(selector).boundingBox();
  if (!box) throw new Error(`no box for ${selector}`);
  await touch(box.x + box.width / 2, box.y + box.height / 2);
}

// Navigate only with goto(): assigning location.href from evaluate() tears down
// the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/dev/components");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

const width = await page.evaluate(() => window.innerWidth);
if (width >= 768) {
  fail(`this device reports ${width} CSS px — the phone assertions need < 768`);
} else {
  ok(`phone width (${width} CSS px, below the md breakpoint)`);
}

const KEBAB = '[data-testid="collection-actions"]';
const MENU = "#context-menu-bench-header-kebab";
const isOpen = () =>
  page.locator(MENU).evaluate((el) => el.matches(":popover-open"));
const aimed = () => page.locator("[data-bench-kebab-aimed]").textContent();
const lastSelected = () =>
  page.locator("[data-bench-kebab-last]").textContent();

await page.locator(KEBAB).scrollIntoViewIfNeeded();

// 1. The 44 px tap target, measured on the engine that will be tapped. The
//    wireframe's mobile kebab is an 18 px glyph; shipping an 18 px *button*
//    would have been a faithful-looking, unusable control.
{
  const box = await page.locator(KEBAB).boundingBox();
  if (!box) fail("the kebab has no box");
  else if (box.width < 44 || box.height < 44) {
    fail(`kebab tap target is ${box.width}×${box.height}, under 44 px`);
  } else {
    ok(`kebab is a ${Math.round(box.width)}×${Math.round(box.height)} tap target`);
  }
  // …and on screen at rest. The tree row's own `⋯` is `opacity-0` until hover at
  // md and up; a header kebab that inherited that would be untappable here and
  // still "visible" to a naive check.
  if ((await page.locator(KEBAB).evaluate((el) => getComputedStyle(el).opacity)) !== "1") {
    fail("the kebab is not on screen at rest");
  } else ok("the kebab is on screen at rest (opacity 1)");
}

// 2. A real tap opens the shared panel, and aims it first.
{
  if ((await aimed()) !== "0") fail("bench state was already aimed — vacuous");
  await tapCenter(KEBAB);
  if (!(await until(isOpen))) {
    fail("a tap on the kebab did not open the menu");
  } else ok("tap on the kebab opens the shared menu (ContextMenuHandle::open_at)");
  if ((await aimed()) !== "1") {
    fail(`the kebab did not aim the menu before opening it (aimed = ${await aimed()})`);
  } else ok("…having aimed the menu at its subject first");
}

// 3. …clamped inside the phone viewport. The header kebab sits lower in the page
//    than a tree row, so `position_at_pointer`'s flip is load-bearing here.
{
  const box = await page.locator(MENU).boundingBox();
  const vp = await page.evaluate(() => ({
    w: window.innerWidth,
    h: window.innerHeight,
  }));
  if (!box) fail("the open menu has no box");
  else if (
    box.x < 0 ||
    box.y < 0 ||
    box.x + box.width > vp.w + 1 ||
    box.y + box.height > vp.h + 1
  ) {
    fail(`menu is off screen: ${JSON.stringify(box)} in ${JSON.stringify(vp)}`);
  } else ok("the panel is clamped inside the phone viewport");
}

// 4. A real tap on an item runs its action and closes the menu. `Move to…` on
//    purpose: it is the action that has no other touch path at all.
{
  if ((await lastSelected()) === "move") {
    fail("bench state was already `move` — vacuous");
  }
  const item = page.locator(`${MENU} [role="menuitem"]`, {
    hasText: "Move to…",
  });
  const box = await item.boundingBox();
  if (!box) fail("the Move to… item has no box");
  else await touch(box.x + box.width / 2, box.y + box.height / 2);
  if (!(await until(async () => !(await isOpen())))) {
    fail("the menu stayed open after an item tap");
  } else ok("tapping an item closes the menu");
  if ((await lastSelected()) !== "move") {
    fail(`item tap did not run on_select (last = ${await lastSelected()})`);
  } else ok("tapping an item runs its on_select");
}

// 5. The rail drawer stayed shut the whole time — the control that says the
//    kebab path does not depend on the tree being on screen, which is the entire
//    reason this affordance exists on a phone.
{
  const rail = page.locator("#sidebar-rail");
  if ((await rail.count()) === 0) {
    ok("no rail on the bench page at all (it is outside AppShell) — control holds trivially");
  } else if (await rail.isVisible()) {
    fail("the rail drawer is on screen at phone width");
  } else ok("the rail drawer stayed off screen at phone width");
}

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

await browser.close();
console.log(
  process.exitCode
    ? "ANDROID HEADER-KEBAB CHECK FAILED"
    : "ANDROID HEADER-KEBAB CHECK PASS",
);
