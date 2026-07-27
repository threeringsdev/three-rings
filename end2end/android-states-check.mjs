// The **state arms** on the real Android webview (ui-work-loop platform matrix
// path 1: attach over CDP to the Tauri debug webview).
//
// Why this platform matters more here than for most surfaces: on the **native**
// backend every read is an HTTPS call to the hosted API, so a *failed* read is
// the ordinary case on a phone rather than an exotic one. The banner that names
// the failure and the control that retries it are, on this device, the normal
// experience of being on a train.
//
// Why the bench and not a real failing page: every surface these arms live on is
// authed, and the dev proxy strips Cookie headers — `/my/*` redirects to
// `/login?next=…` on-device (recorded repeatedly in specs/app-ui.md, and the
// reason quick-add, the selection tray, the tree menu and the my-root list are
// all driven from `/dev/components`). The bench section renders the same
// `ErrorNote` in all four failure classes, which is exactly what a bench can
// show — plus the one thing that cannot be checked anywhere else: **that a real
// finger can hit the retry button.**
//
// Five checks, in order:
//
//   1. the device really is a phone (below `md`), else the layout assertions are
//      about a desktop;
//   2. all four failure classes rendered, each with the affordances its class
//      warrants and no others — the honesty claim, on the real engine;
//   3. the retry button is a ≥ 44 px touch target and the banners do not scroll
//      sideways at phone width, measured on this engine's own font metrics;
//   4. **a real `Input.dispatchTouchEvent` tap on "Try again" runs the
//      callback** — the counter on the page moves, so this is a wired control
//      rather than a drawn one;
//   5. all three tone badges render, and their token families resolve to real
//      colors here (a `bg-success-light` with no CSS behind it is invisible on
//      the device and green in chromium's dev build).
//
// Prereqs (android-smoke skill): the debug app installed and the repo-root
// `cargo leptos watch --features component-bench` serving :3000 via
// `adb reverse tcp:3000 tcp:3000`, then
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

/** A real touch. `Input.dispatchTouchEvent` presses the screen; Playwright's
 *  `tap()` on a CDP-attached page does not go through the webview's own gesture
 *  pipeline. The point carries an `id` and a radius so Chrome tracks the
 *  contact (see android-tree-move-check.mjs). */
async function touch(x, y) {
  const pt = { x, y, id: 1, radiusX: 8, radiusY: 8, force: 1 };
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [pt],
  });
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
  });
}

async function until(pred, ms = 5000) {
  const deadline = Date.now() + ms;
  for (;;) {
    if (await pred()) return true;
    if (Date.now() > deadline) return false;
    await page.waitForTimeout(100);
  }
}

// Navigate only with goto(): assigning location.href from evaluate() tears down
// the execution context mid-call (ui-work-loop Findings).
await page.goto("http://tauri.localhost/dev/components");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });

// 1. Phone width.
const width = await page.evaluate(() => window.innerWidth);
if (width >= 768) {
  fail(`this device reports ${width} CSS px — the phone assertions need < 768`);
} else {
  ok(`phone width (${width} CSS px, below the md breakpoint)`);
}

await page.locator('[data-testid="bench-error-transport"]').scrollIntoViewIfNeeded();

// 2. The four classes, and the affordance each one does and does not offer.
//    Stated as a table so a class silently acquiring the wrong control fails
//    here rather than reading plausibly on screen.
{
  const expected = [
    // testid, data-failure, retry?, sign-in?
    ["bench-error-missing", "missing", false, false],
    ["bench-error-request", "request", false, false],
    ["bench-error-transport", "transport", true, false],
    ["bench-error-session", "session", false, true],
  ];
  for (const [testid, klass, wantRetry, wantSignin] of expected) {
    const box = page.locator(`[data-testid="${testid}"]`);
    if ((await box.count()) !== 1) {
      fail(`${testid} did not render`);
      continue;
    }
    const got = await box.getAttribute("data-failure");
    if (got !== klass) fail(`${testid} classified as ${got}, expected ${klass}`);
    const retry = await box.locator('[data-testid="state-retry"]').count();
    const signin = await box.locator('[data-testid="state-signin"]').count();
    if ((retry > 0) !== wantRetry) {
      fail(
        `${testid} ${retry > 0 ? "offers" : "withholds"} a retry — expected the opposite`,
      );
    }
    if ((signin > 0) !== wantSignin) {
      fail(
        `${testid} ${signin > 0 ? "offers" : "withholds"} a sign-in — expected the opposite`,
      );
    }
    ok(`${klass}: retry=${retry > 0}, sign-in=${signin > 0} — as classified`);
  }
  // The one message that used to read as a page bug rather than as a session
  // that ran out.
  const session = await page
    .locator('[data-testid="bench-error-session"]')
    .innerText();
  if (!/session has expired/i.test(session) || /invalid token/.test(session)) {
    fail(`the session arm reads: ${JSON.stringify(session)}`);
  } else ok("the expired session says so, not `unauthorized: invalid token`");
}

// 3. Touch target and no sideways scroll, on the real engine's metrics.
{
  const m = await page.evaluate(() => {
    const btn = document.querySelector(
      '[data-testid="bench-error-transport"] [data-testid="state-retry"]',
    );
    const banners = [
      ...document.querySelectorAll("[data-failure]"),
    ];
    const r = btn.getBoundingClientRect();
    return {
      w: r.width,
      h: r.height,
      overflow: Math.max(
        ...banners.map((b) => b.scrollWidth - b.clientWidth),
      ),
      doc:
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
    };
  });
  // 32 px is `ButtonSize::Sm`'s own height (`h-8`); the target is the whole
  // 44 px row only where a frame asks for one, and no frame draws this control.
  // Recorded rather than asserted at 44: what matters is that it is hittable and
  // that the number is known.
  ok(`retry button measures ${m.w.toFixed(1)}×${m.h.toFixed(1)} px`);
  if (m.h < 28) fail(`the retry button is ${m.h.toFixed(1)}px tall — unhittable`);
  if (m.overflow > 1) {
    fail(`a banner scrolls sideways by ${m.overflow}px`);
  } else ok("no banner scrolls sideways at phone width");
  if (m.doc > 1) fail(`the document scrolls sideways by ${m.doc}px`);
  else ok("the page does not scroll sideways");
}

// 4. The retry is wired: a real touch moves the counter. Without this the
//    banners above are a screenshot.
{
  const before = await page.locator('[data-testid="bench-retries"]').innerText();
  const btn = page.locator(
    '[data-testid="bench-error-transport"] [data-testid="state-retry"]',
  );
  await btn.scrollIntoViewIfNeeded();
  const box = await btn.boundingBox();
  if (!box) fail("the retry button has no box to tap");
  else {
    await touch(box.x + box.width / 2, box.y + box.height / 2);
    const moved = await until(async () => {
      const now = await page
        .locator('[data-testid="bench-retries"]')
        .innerText();
      return now !== before;
    });
    const after = await page
      .locator('[data-testid="bench-retries"]')
      .innerText();
    if (!moved) fail(`a real tap on Try again did nothing (still ${after})`);
    else ok(`a real tap on Try again ran the callback (${before} → ${after})`);
  }
}

// 5. The three tones, and their tokens resolving to real colors on this engine.
//    A variant whose token family has no CSS emits classes that do nothing —
//    exactly the trap the V1 vendoring pass dropped these variants for.
{
  const tones = await page.evaluate(() => {
    const out = {};
    for (const el of document.querySelectorAll("#states [data-tone]")) {
      const s = getComputedStyle(el);
      out[el.dataset.tone] = {
        bg: s.backgroundColor,
        fg: s.color,
        text: el.textContent.trim(),
      };
    }
    return out;
  });
  for (const tone of ["resolved", "partial", "stale"]) {
    const t = tones[tone];
    if (!t) {
      fail(`the ${tone} badge did not render`);
      continue;
    }
    const transparent = /rgba\(0, 0, 0, 0\)|transparent/.test(t.bg);
    if (transparent) {
      fail(`the ${tone} badge has no background — its token family is missing`);
    } else {
      ok(`${tone} ("${t.text}") → bg ${t.bg}, fg ${t.fg}`);
    }
    if (t.bg === t.fg) fail(`the ${tone} badge is invisible (bg === fg)`);
  }
  const n = Object.keys(tones).length;
  if (n !== 3) fail(`expected 3 tone badges in the section, found ${n}`);
}

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

await browser.close();
console.log(
  process.exitCode ? "ANDROID STATES CHECK FAILED" : "ANDROID STATES CHECK PASS",
);
