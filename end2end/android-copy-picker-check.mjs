// The which-copies picker's rows on the real Android webview (P6-150;
// ui-work-loop platform matrix path 1: attach over CDP to the Tauri debug
// webview).
//
// **What this probe covers, and why it is the bench.** The picker is where a
// batch move's quantity and version are chosen, and it is a *mobile* control:
// a `− n +` per row, sized for a thumb, inside a panel that has to fit a phone.
// Every page that can open the real dialog is authed, and the dev proxy strips
// Cookie headers, so on-device they bounce to `/login?next=…` (app-ui
// Findings; the tray, needs, states and tap-target probes all run through the
// bench for the same reason — verified again for this task: a real sign-in
// attempt inside the webview answers "Something went wrong"). The bench section
// mounts `PickerRows` — the dialog's own rows, markup and testids — over
// synthetic stacks.
//
// So the four platform-risky things are asserted here and cannot be asserted
// anywhere else:
//
//   1. the rows render at all on this engine, split to the full grain, with the
//      labels that are the only thing telling two stacks of one binder apart;
//   2. the ± buttons are real 44 px targets on the device's own
//      `devicePixelRatio` and font scaling — this control deliberately does not
//      reuse `CountStepper`, whose ± are `hidden sm:inline-flex` and would be
//      *absent* here;
//   3. a **real touch** on `+` raises that row's count (Playwright's tap() is
//      unavailable over CDP — no `hasTouch` — and a click() would not prove a
//      touch lands);
//   4. the row list does not scroll sideways at phone width.
//
// Prereqs (android-smoke skill): the leptos watch server on :3000 with
//   adb reverse tcp:3000 tcp:3000
// the debug app launched, and
//   socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | tail -1)
//   adb forward tcp:9222 "localabstract:$socket"
// (re-discover the socket on every launch — it embeds the app pid).
import { chromium } from "playwright";

const TAP = 44;
const fail = (msg) => {
  console.error(`FAIL: ${msg}`);
  process.exitCode = 1;
};
const ok = (msg) => console.log(`  ok  ${msg}`);

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const page = browser.contexts()[0].pages()[0];
const pageErrors = [];
page.on("pageerror", (e) => pageErrors.push(String(e).slice(0, 200)));

await page.goto("http://tauri.localhost/dev/components");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

const metrics = await page.evaluate(() => ({
  dpr: window.devicePixelRatio,
  w: window.innerWidth,
  h: window.innerHeight,
}));
console.log(`  viewport ${metrics.w}×${metrics.h} css px · dpr ${metrics.dpr}`);

const section = page.locator('[data-testid="bench-copy-picker"]');
await section.scrollIntoViewIfNeeded();
const rows = section.locator('[data-testid="which-copies-row"]');

// 1. The rows, at full grain.
const count = await rows.count();
if (count === 4) ok("four full-grain rows render");
else fail(`expected 4 rows, got ${count}`);

const labels = await rows.allInnerTexts();
const label = (i) => labels[i].replace(/\s+/g, " ").trim();
if (label(1).includes("Trade Binder") && label(1).includes("2 copies"))
  ok(`plain stack reads "${label(1)}"`);
else fail(`plain stack label: ${label(1)}`);
if (label(2).includes("foil") && label(2).includes("MH3 #123"))
  ok(`the foil stack of the same binder is its own row: "${label(2)}"`);
else fail(`foil row lost its grain: ${label(2)}`);
if (label(3).includes("sideboard") && label(3).includes("LP") && label(3).includes("JA"))
  ok(`board, condition and language all name themselves: "${label(3)}"`);
else fail(`grain parts missing: ${label(3)}`);
// A card the read found nothing for keeps its section and says so.
if ((await section.locator("text=No copies left to move").count()) === 1)
  ok("an emptied card keeps its section with a plain sentence");
else fail("the emptied card's section is missing or duplicated");

// 2. Touch targets, in CSS px on the real engine.
const dec = await rows.first().locator('[data-testid="pick-dec"]').boundingBox();
const inc = await rows.first().locator('[data-testid="pick-inc"]').boundingBox();
console.log(`  − ${dec?.width}×${dec?.height} · + ${inc?.width}×${inc?.height}`);
for (const [name, box] of [["−", dec], ["+", inc]]) {
  if (box && box.width >= TAP && box.height >= TAP)
    ok(`${name} is ${TAP}+ px on the real engine`);
  else fail(`${name} is under ${TAP} px on-device: ${JSON.stringify(box)}`);
}

// 3. A real touch on `+`.
const cdp = await page.context().newCDPSession(page);
async function tap(x, y) {
  const pt = { x, y, id: 1, radiusX: 8, radiusY: 8, force: 1 };
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [pt] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
}
async function until(pred, ms = 3000) {
  const deadline = Date.now() + ms;
  for (;;) {
    if (await pred()) return true;
    if (Date.now() > deadline) return false;
    await page.waitForTimeout(100);
  }
}

const value = rows.first().locator('[data-testid="pick-value"]');
const total = page.locator('[data-testid="bench-copy-picker-total"]');
if ((await value.innerText()) === "1") ok("every row opens at one copy");
else fail(`row 1 opened at ${await value.innerText()}`);
if ((await total.innerText()) === "Move 4 copies") ok("the total names what a confirm would move");
else fail(`total reads ${await total.innerText()}`);

await tap(inc.x + inc.width / 2, inc.y + inc.height / 2);
if (await until(async () => (await value.innerText()) === "2"))
  ok("a real touch on + raises that row's count");
else fail(`+ did not respond to a touch (value ${await value.innerText()})`);
if (await until(async () => (await total.innerText()) === "Move 5 copies"))
  ok("the total follows the row");
else fail(`total did not follow: ${await total.innerText()}`);

// The ceiling is the stack: four copies, so `+` stops there.
for (let i = 0; i < 3; i++) {
  await tap(inc.x + inc.width / 2, inc.y + inc.height / 2);
  await page.waitForTimeout(150);
}
if (await until(async () => (await value.innerText()) === "4"))
  ok("the stepper stops at the stack's own size");
else fail(`stepper passed its ceiling: ${await value.innerText()}`);
if (await rows.first().locator('[data-testid="pick-inc"]').isDisabled())
  ok("+ is disabled at the ceiling");
else fail("+ is still enabled past the stack size");

// 4. No sideways scroll in the row list at phone width.
const overflow = await page.evaluate(() => {
  const p = document.querySelector('[data-testid="bench-copy-picker"]');
  return {
    page: document.documentElement.scrollWidth - window.innerWidth,
    rows: p ? p.scrollWidth - p.clientWidth : -1,
  };
});
if (overflow.rows <= 0) ok("the row list does not scroll sideways");
else fail(`the row list overflows by ${overflow.rows} px`);

if (pageErrors.length === 0) ok("no page errors");
else fail(`page errors: ${pageErrors.join(" | ")}`);

await browser.close();
console.log(process.exitCode ? "FAILED" : "PASS");
