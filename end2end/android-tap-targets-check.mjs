// Touch-target geometry on the real Android webview (ui-work-loop platform
// matrix path 1: attach over CDP to the Tauri debug webview).
//
// The responsive audit's headline fix is a 44 px tap target where there used to
// be a 16 px one, and **a 44 px claim is only worth something on the engine that
// will be taking the finger.** Chromium's 390×844 emulation gets the CSS right;
// what it cannot tell you is how this WebView resolves `size-11` against the
// device's own `devicePixelRatio` and default font scaling, or whether the padded
// wrapper survives its layout at all.
//
// Everything measured here is measured in **CSS pixels**, which is the unit the
// 44 px guideline is written in — the probe prints `devicePixelRatio` alongside
// so a physical-size question can still be answered from the output.
//
// Why the bench: every surface that hosts a real row checkbox (`/my`,
// `/my/all`, `/my/collections/:id`) is authed, and the dev proxy strips Cookie
// headers, so on-device they redirect to `/login?next=…` (app-ui Findings). The
// bench mounts the same `SelectionCheckbox` the pages do. The two pieces of shell
// chrome measured at the end — the rail toggle and the tab bar — only exist
// inside `AppShell`, so they are measured on `/catalog`, which is public.
//
// Prereqs (android-smoke skill): the leptos watch server on :3000 with
//   adb reverse tcp:3000 tcp:3000
// the debug app launched, and
//   socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | tail -1)
//   adb forward tcp:9222 "localabstract:$socket"
// (re-discover the socket on every launch — it embeds the app pid, and a stale
// one forwards to a dead process and lists no targets at all).
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

const metrics = await page.evaluate(() => ({
  dpr: window.devicePixelRatio,
  w: window.innerWidth,
  h: window.innerHeight,
  ua: navigator.userAgent.match(/Chrome\/[\d.]+/)?.[0] ?? "?",
}));
console.log(
  `viewport ${metrics.w}×${metrics.h} CSS px · devicePixelRatio ${metrics.dpr} · ${metrics.ua}`,
);
// The threshold that matters is Tailwind's `md` (768 px), not 390: the 44 px
// targets are the *un-prefixed* arm and every `md:` override switches them back
// to desktop sizes. This AVD reports 540 CSS px, which is below `md`, so the
// mobile arm is the one being measured. Assert that rather than a device width.
if (metrics.w < 768) ok(`viewport is below md (${metrics.w} CSS px) — the touch arm is live`);
else
  fail(
    `viewport is ${metrics.w} CSS px, at or above md — the md: overrides are active and these targets are the desktop ones`,
  );

// Navigate only with goto(): assigning location.href from evaluate() tears down
// the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/dev/components");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

// ------------------------------------- the row select target on the bench ---

const target = page.locator('#bench-tray-rows [data-testid="row-select-target"]');
const boxCtl = page.locator('#bench-tray-rows [data-testid="row-select"]');
await target.first().scrollIntoViewIfNeeded();

if ((await target.count()) === 4) ok("bench renders four select targets");
else fail(`expected 4 select targets, got ${await target.count()}`);

const t = await target.first().boundingBox();
const b = await boxCtl.first().boundingBox();
console.log(
  `  select target ${t?.width}×${t?.height} · drawn box ${b?.width}×${b?.height}`,
);
if (t && t.width >= TAP && t.height >= TAP)
  ok(`select target is ${TAP}+ px on the real engine`);
else fail(`select target under ${TAP} px on-device: ${JSON.stringify(t)}`);
// The point of the padded wrapper is that the *drawn* checkbox stays the
// wireframe's small box — a 44 px checkbox would pass the line above and be the
// wrong change.
if (b && b.width <= 20 && b.height <= 20)
  ok("the drawn checkbox is still the small box");
else fail(`the drawn checkbox grew: ${JSON.stringify(b)}`);

// A real touch, at a coordinate inside the padded ring and outside the drawn
// box — the corner that used to be dead space. `Input.dispatchTouchEvent`
// rather than `locator.click()`: a CDP-attached context has no `hasTouch`, so
// Playwright's tap() throws, and a click() would not prove a *touch* lands here.
const tray = page.locator('[data-testid="selection-tray"]');
if ((await tray.count()) === 0) ok("no tray at zero selection");
else fail("tray rendered with an empty selection");

const cdp = await page.context().newCDPSession(page);
/** A real touch. The point carries an `id` and a radius because without them
 *  Chrome's gesture pipeline does not track the contact across the two events. */
async function tap(x, y) {
  const pt = { x, y, id: 1, radiusX: 8, radiusY: 8, force: 1 };
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [pt] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
}
/** Poll, never sleep: the webview synthesizes the click from the touch on its
 *  own schedule, and a fixed wait either flakes or hides how long it took. A
 *  250 ms wait reported this very probe's ring tap as "did nothing". */
async function until(pred, ms = 3000) {
  const deadline = Date.now() + ms;
  for (;;) {
    if (await pred()) return true;
    if (Date.now() > deadline) return false;
    await page.waitForTimeout(100);
  }
}

const cornerX = t.x + 3;
const cornerY = t.y + 3;
if (cornerX < b.x - 2 || cornerY < b.y - 2)
  ok("the tap coordinate is outside the drawn box");
else
  fail(
    `corner (${cornerX},${cornerY}) is not outside the box at (${b.x},${b.y}) — the tap proves nothing`,
  );
await tap(cornerX, cornerY);

if (await until(async () => (await tray.count()) === 1))
  ok("a real touch on the padded ring selects the row");
else fail("a real touch on the padded ring did nothing");
if ((await boxCtl.first().getAttribute("aria-checked")) === "true")
  ok("the row announces aria-checked=true after the ring tap");
else fail("the ring tap did not check the row");

// Positive control: a touch just *outside* the target must NOT select, or the
// assertion above would pass for a page-wide handler.
await page.locator('[data-testid="tray-clear"]').click();
if (await until(async () => (await tray.count()) === 0)) ok("clear emptied the tray");
else fail("tray survived clear");
await tap(t.x + t.width + 12, t.y + t.height + 30);
// Waited out over the same window the positive tap is allowed, so "nothing
// happened" means nothing happened rather than nothing happened *yet*.
if (!(await until(async () => (await tray.count()) === 1)))
  ok("a touch outside the target does not select (positive control)");
else fail("a touch outside the target selected a row — the target is not bounded");

// ------------------------------------------------ shell chrome, on /catalog ---

await page.goto("http://tauri.localhost/catalog");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

// The bottom tab bar: the toaster's phone-width offset is measured against its
// height, so the height is worth reading on the real engine rather than assuming
// the 59 px chromium reports.
const tabs = await page.locator('nav[aria-label="Primary"]').boundingBox();
const vh = metrics.h;
console.log(`  tab bar ${tabs?.width}×${tabs?.height} at y=${tabs?.y} (viewport h=${vh})`);
if (tabs && tabs.height > 0 && Math.abs(tabs.y + tabs.height - vh) <= 2)
  ok("the tab bar is docked to the viewport floor");
else fail(`tab bar not at the floor: ${JSON.stringify(tabs)} vs h=${vh}`);
for (const name of ["Catalog", "My cards"]) {
  const tab = await page
    .locator(`nav[aria-label="Primary"] a`, { hasText: name })
    .first()
    .boundingBox();
  if (tab && tab.height >= TAP) ok(`the ${name} tab is ${TAP}+ px tall`);
  else fail(`the ${name} tab is under ${TAP} px: ${JSON.stringify(tab)}`);
}

// The toaster must clear the tab bar. This is the arm that was broken with no
// selection at all, and the shell renders here even anonymously.
const toaster = await page.locator('[data-name="Toaster"]').boundingBox();
if (toaster && tabs && toaster.y + toaster.height <= tabs.y + 1)
  ok(`the toaster clears the tab bar by ${Math.round(tabs.y - (toaster.y + toaster.height))} px`);
else
  fail(
    `a toast would paint over the tab bar: toaster ${JSON.stringify(toaster)} vs tabs y=${tabs?.y}`,
  );

// The rail toggle only renders in My-cards mode, which is authed — so on-device
// it is reachable only as markup on the redirected page. Assert its absence here
// (Catalog mode deliberately keeps its one designed mobile filter path) and its
// size on the bench's own header-kebab section, which uses the same `size-11`.
if ((await page.locator('[data-testid="rail-toggle"]').count()) === 0)
  ok("Catalog mode does not offer a second rail entry point");
else fail("the rail toggle leaked into Catalog mode");

// Zero sideways scroll on the real engine, document *and* every scroll
// container — the audit's own kill-verification subject.
const overflow = await page.evaluate(() => ({
  doc: document.documentElement.scrollWidth - document.documentElement.clientWidth,
  wrappers: [...document.querySelectorAll('[data-name="TableWrapper"]')]
    .filter((w) => w.clientWidth > 0)
    .map((w) => w.scrollWidth - w.clientWidth),
}));
if (overflow.doc <= 1 && overflow.wrappers.every((n) => n <= 1))
  ok(`no sideways scroll (doc=${overflow.doc}, wrappers=[${overflow.wrappers}])`);
else fail(`sideways scroll on-device: ${JSON.stringify(overflow)}`);

if (pageErrors.length === 0) ok("no page errors");
else fail(`page errors: ${pageErrors.join(" | ")}`);

await browser.close();
console.log(
  process.exitCode ? "ANDROID TAP-TARGETS CHECK FAIL" : "ANDROID TAP-TARGETS CHECK PASS",
);
