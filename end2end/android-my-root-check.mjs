// The My-cards **root drill-down list** on the real Android webview
// (ui-work-loop platform matrix path 1: attach over CDP to the Tauri debug
// webview). This is the touch half of the "mobile /my root" task.
//
// Why the bench and not `/my`: the list is authed and the dev proxy strips
// Cookie headers, so `/my` redirects to `/login?next=/my` on-device — recorded
// three times over in specs/app-ui.md Findings, and the reason quick-add, the
// selection tray and the tree's touch menu are all driven on `/dev/components`.
// The bench section (`app/src/bench/my_root.rs`) renders the same `MyRootList`
// over a fixed tree, and the thing under test is exactly what a bench can show:
// **a real touch on a row navigates**, at the frame's own metrics.
//
// Five things are checked, in order:
//
//   1. the device really is below the `md` breakpoint (else every assertion
//      below is about the desktop layout);
//   2. the list rendered the frame's shape — `All cards` first, the Inbox
//      pinned above the other collections, `Shopping list` last, and nothing
//      nested;
//   3. rows are ≥ 44 px tall and do not overflow their container sideways,
//      measured on the real engine's font metrics rather than chromium's;
//   4. **a real `Input.dispatchTouchEvent` tap on a row navigates to that
//      row's `href`.** The row is an `<a>`, so this is the whole drill-down;
//      the destination is authed, so the webview lands on `/login?next=…`
//      through the redirect-swallowing shim — which is itself the proof the tap
//      reached the link and a real navigation happened;
//   5. the anonymous `/my` guard bounce still works on this platform (the
//      `data-ssr-path` shim, not a browser-followed 302), because `/my` now has
//      a second page component in it.
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

const width = await page.evaluate(() => window.innerWidth);
if (width >= 768) {
  fail(`this device reports ${width} CSS px — the phone assertions need < 768`);
} else {
  ok(`phone width (${width} CSS px, below the md breakpoint)`);
}

const ROW = '[data-testid="my-root-row"]';
await page.locator('[data-testid="my-root-list"]').scrollIntoViewIfNeeded();

// 2. The frame's shape, rendered by the real engine.
{
  const labels = await page.locator(`${ROW} span:nth-child(2)`).allInnerTexts();
  const expected = [
    "All cards",
    "Inbox",
    "Binders",
    "Decks",
    "Shopping list",
  ];
  if (JSON.stringify(labels) !== JSON.stringify(expected)) {
    fail(`rows are ${JSON.stringify(labels)}, expected ${JSON.stringify(expected)}`);
  } else {
    ok("All cards · Inbox (pinned) · collections · Shopping list");
  }
  // The fixture nests `Trade`/`Bulk` under Binders and `Grixis` under Decks:
  // a flattened list would show them, a drill-down must not.
  const flat = labels.filter((l) => ["Trade", "Bulk", "Grixis"].includes(l));
  if (flat.length) fail(`nested collections leaked to the root: ${flat}`);
  else ok("nested collections stay one level down");
}

// 3. Touch targets and no sideways scroll, on the real engine's metrics.
{
  const m = await page.evaluate((sel) => {
    const list = document.querySelector('[data-testid="my-root-list"]');
    const rows = [...document.querySelectorAll(sel)];
    return {
      minHeight: Math.min(...rows.map((r) => r.getBoundingClientRect().height)),
      count: rows.length,
      overflow: list.scrollWidth - list.clientWidth,
      client: list.clientWidth,
    };
  }, ROW);
  if (m.count < 5) fail(`only ${m.count} rows rendered`);
  if (m.minHeight < 44) {
    fail(`shortest row is ${m.minHeight.toFixed(1)}px, under the 44px target`);
  } else ok(`every row is ≥ 44px tall (shortest ${m.minHeight.toFixed(1)}px)`);
  if (m.overflow > 1) {
    fail(`the list scrolls sideways by ${m.overflow}px in ${m.client}px`);
  } else ok(`the list does not scroll sideways (${m.client}px wide)`);
}

// 4. The drill-down itself: a real tap on a row navigates to its href. The
//    target is authed, so the app bounces to `/login?next=<href>` — which is
//    what makes this a positive result, not a silent no-op.
{
  const row = page.locator(ROW).nth(2); // "Binders" — a collection row
  const href = await row.getAttribute("href");
  if (!href || !href.startsWith("/my/collections/")) {
    fail(`row 3 has an unexpected href: ${href}`);
  }
  const box = await row.boundingBox();
  if (!box) fail("the row has no box to tap");
  else {
    await touch(box.x + box.width / 2, box.y + box.height / 2);
    const landed = await until(async () => {
      const u = await page.evaluate(() => location.pathname + location.search);
      return u !== "/dev/components";
    });
    const url = await page.evaluate(() => location.pathname + location.search);
    if (!landed) {
      fail(`a real tap on the row did not navigate (still at ${url})`);
    } else if (
      url !== href &&
      url !== `/login?next=${encodeURIComponent(href).replace(/%2F/g, "/")}`
    ) {
      fail(`tap navigated to ${url}, expected ${href} or its login bounce`);
    } else {
      ok(`a real tap on a row drills in (→ ${url})`);
    }
  }
}

// 5. `/my` itself: two page components now live in that route, and on this
//    platform the anonymous guard travels through the `data-ssr-path` shim
//    rather than a browser-followed 302.
await page.goto("http://tauri.localhost/my");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
{
  const url = await page.evaluate(() => location.pathname + location.search);
  if (url !== "/login?next=/my") {
    fail(`anonymous /my landed on ${url}, expected /login?next=/my`);
  } else ok("anonymous /my still bounces to /login?next=/my through the shim");
  // Positive control: the login form actually rendered, so the assertion above
  // is about the redirect and not about a blank page.
  if (!(await page.locator("input[name=email]").isVisible())) {
    fail("the login form did not render — control failed");
  } else ok("…and the login form is on screen (control)");
}

if (pageErrors.length) fail(`page errors: ${pageErrors.join(" | ")}`);
else ok("no page errors");

await browser.close();
console.log(
  process.exitCode ? "ANDROID MY-ROOT CHECK FAILED" : "ANDROID MY-ROOT CHECK PASS",
);
