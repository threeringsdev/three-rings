// Probe: the card-detail surfaces on the real Android webview, over CDP.
//
// The Android leg of the card-detail task (specs/app-ui.md → "`/cards/:id`").
// Playwright's desktop projects emulate touch; this is the only place the
// *actual* Android WebView decides `(pointer: coarse)`, so it is what proves
// the tap-opens-a-sheet branch rather than assuming an emulated pointer type.
//
// Prereqs — the android-smoke skill's dev-attach recipe:
//   1. app running on the emulator (`cargo tauri android dev` from repo root)
//   2. socket=$(adb shell "cat /proc/net/unix" | grep -ao 'webview_devtools_remote_[0-9]*' | head -1)
//   3. adb forward tcp:9222 localabstract:$socket
//
// Usage: node android-card-detail-check.mjs [port]
import { chromium } from "@playwright/test";

const port = process.argv[2] ?? "9222";
const ORIGIN = "http://tauri.localhost";
const failures = [];

function check(name, ok, detail = "") {
  console.log(`${ok ? "ok  " : "FAIL"} — ${name}${detail ? `: ${detail}` : ""}`);
  if (!ok) failures.push(name);
}

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
  timeout: 15000,
});
const page = browser.contexts().flatMap((c) => c.pages())[0];
if (!page) {
  console.error("ANDROID CARD DETAIL FAIL: no page in the webview");
  process.exit(1);
}

// Always goto an explicit URL — one shared page/context across runs, and JS
// location.href races the CDP session (ui-work-loop Findings).
async function go(path) {
  await page.goto(`${ORIGIN}${path}`, { waitUntil: "domcontentloaded" });
  await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
}

// The real WebView's own answer, not an emulated one.
await go("/catalog?q=Lightning%20Bolt");
const coarse = await page.evaluate(
  () => matchMedia("(pointer: coarse)").matches,
);
check("the Android webview reports a coarse pointer", coarse === true);

// --- tap a tile: sheet opens, navigation suppressed
await page.getByTestId("card-preview-trigger").first().click();
const sheet = page.locator("[data-testid=card-preview-sheet][role=dialog]");
// `data-state`, not visibility: the sheet slides in via a transform and stays
// in the layout when closed, so a closed sheet is "visible" too.
await sheet.waitFor({ state: "attached", timeout: 5000 }).catch(() => {});
await page.waitForTimeout(400);
check(
  "tapping a tile opens the bottom sheet",
  (await sheet.getAttribute("data-state")) === "open",
  await sheet.getAttribute("data-state"),
);
check(
  "the sheet names the card",
  (await sheet.textContent().catch(() => ""))?.includes("Lightning Bolt"),
);
check(
  "the tap did not navigate",
  new URL(page.url()).pathname === "/catalog",
  page.url(),
);

// The hover card must NOT also be up — a synthetic mouseenter accompanies the
// tap, which is exactly what the hover_card `disabled` prop suppresses.
await page.waitForTimeout(400);
const hover = page.locator("[data-testid=card-preview-hover]");
check(
  "no hover card stacked on top of the sheet",
  (await hover.count()) === 0 || !(await hover.first().isVisible()),
);

// --- "Full details →" is the way through to the page
await sheet.getByTestId("card-preview-full-details").click();
await page
  .waitForURL((u) => u.pathname.startsWith("/cards/"), { timeout: 10000 })
  .catch(() => {});
check(
  "Full details navigates to the card page",
  new URL(page.url()).pathname.startsWith("/cards/"),
  page.url(),
);

// --- the detail page itself, rendered by the real WebView
const name = await page
  .getByTestId("card-name")
  .textContent()
  .catch(() => null);
check("the detail page renders the card name", name?.includes("Lightning Bolt"));
check(
  "printings render",
  await page.locator("[data-testid=card-printings]").isVisible(),
);

// --- the multi-face image fix, on-device
const dfc = await page.evaluate(async () => {
  const res = await fetch("/api/search_catalog?q=Agadeem%27s%20Awakening");
  return (await res.json()).cards[0];
});
check(
  "a multi-face card carries an image",
  typeof dfc?.image_uri === "string" && dfc.image_uri.startsWith("https://"),
  dfc?.name,
);
await go(`/cards/${dfc.oracle_id}`);
const heroSrc = await page
  .locator("img")
  .first()
  .getAttribute("src")
  .catch(() => null);
check(
  "the multi-face detail page renders its art",
  typeof heroSrc === "string" && heroSrc.includes("scryfall"),
  heroSrc ?? "no img",
);

const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.waitForTimeout(300);
check("no page errors", errors.length === 0, errors.join("; "));

await browser.close();
if (failures.length) {
  console.error(`\nANDROID CARD DETAIL FAIL (${failures.length}): ${failures.join(", ")}`);
  process.exit(1);
}
console.log("\nANDROID CARD DETAIL PASS");
