// Collection-tree *management* task on the Android webview (ui-work-loop
// platform matrix path 1: attach over CDP to the Tauri debug webview).
//
// The tree management surface is authed and the dev proxy strips Cookie
// headers (ui-work-loop Findings), so on-device coverage is the newly
// vendored `context_menu` on the bench — the real-webview check the
// vendor-component checklist asks for, and the one that matters most here
// because the component switched to a native `popover="manual"` with
// custom pointer dismissal, and Android long-press synthesizes `contextmenu`.
//
// Prereqs (android-smoke skill): `cargo tauri android dev` running (or the
// debug app already installed + the watch serving :3000 via adb reverse), and
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

// Navigate only with goto(): assigning location.href from evaluate() tears
// down the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/dev/components");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

const menuSel = "#context-menu-bench-context-menu";
const isOpen = () =>
  page.locator(menuSel).evaluate((el) => el.matches(":popover-open"));

// A long-press on a touch webview synthesizes `contextmenu`; dispatch it at a
// point so we also exercise the viewport-clamp positioning path on the real
// engine.
async function openAt(x, y) {
  await page.locator("[data-bench-context-target]").evaluate(
    (el, { x, y }) =>
      el.dispatchEvent(
        new PointerEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: x,
          clientY: y,
        }),
      ),
    { x, y },
  );
}

// 1. Opens on contextmenu and enters the top layer.
await page.locator("[data-bench-context-target]").scrollIntoViewIfNeeded();
await openAt(40, 200);
await page.waitForTimeout(200);
if (!(await isOpen())) fail("context_menu did not open on the Android webview");
else ok("context_menu opens (native popover=manual, top layer)");

// 2. Positioned on-screen (left/top set, within the viewport).
const placed = await page.locator(menuSel).evaluate((el) => {
  const r = el.getBoundingClientRect();
  return {
    inViewport:
      r.left >= 0 &&
      r.top >= 0 &&
      r.right <= window.innerWidth + 1 &&
      r.bottom <= window.innerHeight + 1,
    hasSize: r.width > 0 && r.height > 0,
  };
});
if (!placed.hasSize) fail("context_menu has no box on the device");
else if (!placed.inViewport) fail("context_menu positioned off-screen");
else ok("context_menu is positioned within the viewport");

// 3. Tapping a menu item runs its action and closes.
await page
  .locator(`${menuSel} [role="menuitem"]`, { hasText: "Rename…" })
  .click();
await page.waitForTimeout(200);
if (await isOpen()) fail("context_menu stayed open after an item tap");
else ok("item tap runs the action and closes");
if ((await page.locator("[data-bench-context-last]").textContent()) !== "rename")
  fail("context_menu item did not run its on_select on the device");
else ok("item on_select fired");

// 4. Outside tap dismisses (the custom pointerdown light-dismiss).
await openAt(40, 200);
await page.waitForTimeout(200);
if (!(await isOpen())) fail("context_menu re-open failed");
await page.mouse.click(5, 5);
await page.waitForTimeout(200);
if (await isOpen()) fail("outside tap did not dismiss on the device");
else ok("outside tap dismisses");

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

console.log(
  process.exitCode ? "ANDROID TREE-MANAGE CHECK FAIL" : "ANDROID TREE-MANAGE CHECK PASS",
);
await browser.close();
