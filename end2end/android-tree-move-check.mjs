// The tree's mouse-free move path on the **real Android webview**
// (ui-work-loop platform matrix path 1: attach over CDP to the Tauri debug
// webview). This is the touch half of the `Move to…` task.
//
// Why the bench and not `/my`: the tree is authed and the dev proxy strips
// Cookie headers (ui-work-loop Findings — re-measured for this task, `/my`
// still redirects to `/login?next=/my` on-device), so the real surface is
// unreachable here. The two things the touch path actually depends on are
// primitives, and both live on the bench:
//
//   1. a **tap** on an `⋯`-shaped button opening the shared panel through
//      `ContextMenuHandle::open_at` — exactly what a tree row does — and a
//      **tap** on an item running its action and closing the menu;
//   2. a real **long-press** synthesizing `contextmenu`. That claim has been
//      carried in this repo's comments since the context_menu was vendored
//      but was never measured: the previous Android check *dispatched a
//      synthetic `contextmenu` event* rather than pressing. It is measured
//      here, and reported either way — the tap path above is what the feature
//      actually relies on, so a long-press that does not fire is a finding,
//      not a failure.
//
// Plus one regression check on the real engine for the rail's new responsive
// shape (`invisible` / `fixed` / `transition-[left]` / `data-[open=true]`
// instead of `hidden md:block`): at phone width the sidebar must still not be
// on screen in Catalog mode.
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
const note = (msg) => console.log(`  ..  ${msg}`);

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const page = browser.contexts()[0].pages()[0];
const pageErrors = [];
page.on("pageerror", (e) => pageErrors.push(String(e).slice(0, 200)));

const cdp = await page.context().newCDPSession(page);

/** A real touch, held. `Input.dispatchTouchEvent` is the only way to press
 *  rather than click — Playwright's `tap()` releases immediately. The point
 *  carries an `id` and a radius: without them Chrome's gesture pipeline does
 *  not track the contact across events, which is what a long-press is. */
async function touch(x, y, holdMs) {
  const pt = { x, y, id: 1, radiusX: 8, radiusY: 8, force: 1 };
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [pt],
  });
  if (holdMs) await page.waitForTimeout(holdMs);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
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

async function centerOf(selector) {
  const box = await page.locator(selector).boundingBox();
  if (!box) throw new Error(`no box for ${selector}`);
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

// Navigate only with goto(): assigning location.href from evaluate() tears
// down the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/dev/components");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

const width = await page.evaluate(() => window.innerWidth);
if (width >= 768) {
  fail(`this device reports ${width} CSS px — the phone assertions need < 768`);
} else {
  ok(`phone width (${width} CSS px, below the md breakpoint)`);
}

const menuSel = "#context-menu-bench-context-menu";
const isOpen = () =>
  page.locator(menuSel).evaluate((el) => el.matches(":popover-open"));
const lastSelected = () =>
  page.locator("[data-bench-context-last]").textContent();

await page.locator("[data-bench-context-tap]").scrollIntoViewIfNeeded();

// 1. A real tap on the `⋯` button opens the shared panel — the tree row's own
//    trigger, and the only one a phone has (there is no right-click, and the
//    hover-reveal at md and up does not apply below it).
{
  const p = await centerOf("[data-bench-context-tap]");
  await touch(p.x, p.y);
  if (!(await until(isOpen))) fail("a tap on the ⋯ button did not open the menu");
  else ok("tap on ⋯ opens the shared menu (ContextMenuHandle::open_at)");
}

// 2. …anchored on screen. `open_at` is handed the button's rect, and the
//    panel's own clamp keeps it inside the viewport — a menu that opens off
//    the bottom of a phone is a menu you cannot use.
{
  const box = await page.locator(menuSel).boundingBox();
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
    fail(
      `menu is off screen: ${JSON.stringify(box)} in ${JSON.stringify(vp)}`,
    );
  } else ok("the panel is clamped inside the phone viewport");
}

// 3. A real tap on an item runs its action and closes the menu. This is the
//    step the whole touch path ends on ("Move to…" → the picker).
{
  const before = await lastSelected();
  if (before === "rename") fail("bench state was already `rename` — vacuous");
  const item = page.locator(`${menuSel} [role="menuitem"]`, {
    hasText: "Rename…",
  });
  const box = await item.boundingBox();
  await touch(box.x + box.width / 2, box.y + box.height / 2);
  if (!(await until(async () => !(await isOpen())))) {
    fail("the menu stayed open after an item tap");
  } else ok("tapping an item closes the menu");
  if ((await lastSelected()) !== "rename") {
    fail(`item tap did not run on_select (last = ${await lastSelected()})`);
  } else ok("tapping an item runs its on_select");
}

// 4. Long-press → `contextmenu`. Measured, not assumed (see the header).
{
  const p = await centerOf("[data-bench-context-target]");
  await touch(p.x, p.y, 1200);
  if (await until(isOpen, 2000)) {
    ok("long-press synthesizes contextmenu on this webview");
    await page.keyboard.press("Escape");
    await page.waitForTimeout(150);
  } else {
    note(
      "long-press did NOT synthesize contextmenu here — the tap trigger above " +
        "is what the touch path relies on, so this is a recorded fact, not a " +
        "failure",
    );
  }
}

// 5. The rail's new responsive shape, on the real engine: at phone width in
//    Catalog mode the sidebar must be off screen and non-interactive, exactly
//    as `hidden md:block` used to make it. The `Filters` sheet trigger is the
//    positive control — it proves the page rendered its mobile chrome at all.
await page.goto("http://tauri.localhost/catalog");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
{
  const rail = page.locator("#sidebar-rail");
  if ((await rail.count()) !== 1) fail("the sidebar rail is not in the DOM");
  else if (await rail.isVisible()) {
    fail("the closed rail drawer is on screen at phone width");
  } else ok("the closed rail drawer is off screen at phone width");
  if (!(await page.getByRole("button", { name: /Filters/ }).isVisible())) {
    fail("the mobile Filters trigger is missing — control failed");
  } else ok("…while Catalog's own mobile filter trigger is there (control)");
}

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

await browser.close();
console.log(
  process.exitCode ? "ANDROID TREE-MOVE CHECK FAILED" : "ANDROID TREE-MOVE CHECK PASS",
);
