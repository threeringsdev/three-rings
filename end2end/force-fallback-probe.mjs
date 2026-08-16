// Ad-hoc isolated probe (not a test) for WB-01M05K42G96ZSGHEBHKQ47CBDV: the
// chromium tier this suite runs cannot exercise `popover.rs`'s JS
// positioning fallback at all — chromium supports CSS anchor positioning
// natively, so `anchor_positioning_supported()` always reads true there.
// This forces the fallback to run in chromium anyway, matching #148's own
// verification precedent ("the JS fallback in isolation — anchor-only CSS
// stripped, matching what a non-supporting engine's parser actually does"):
//
//   1. `page.addInitScript` overrides `window.CSS.supports` so the exact
//      probe string the app's own `anchor_positioning_supported()` queries
//      (`"position-anchor: --x"`) reads false — this is what flips the Rust
//      `if want_open && !anchor_positioning_supported()` gate into the
//      fallback branch (`apply_fallback_position` + `watch_panel_resize`).
//   2. The same init script injects `[popover] { position-anchor: none
//      !important; }` — with no anchor associated, every `anchor()`
//      reference and every `position-area` value in the per-align CSS goes
//      invalid-at-computed-value-time and falls back to that property's own
//      initial value, exactly what a genuinely non-supporting engine's
//      parser would already have discarded. What's left on screen is
//      controlled entirely by the JS fallback's own inline `position:
//      fixed; left; top` (see the tuning note below for why this rule is
//      scoped to `position-anchor` only, not also `inset`).
//
// Two surfaces, matching the maintainer's own screenshots:
//   - the tray's End-aligned, bottom-docked "Move to…" picker (must open
//     ABOVE, right edges aligned, fully inside the viewport)
//   - the catalog's Center-aligned "Adding to" picker (must center over its
//     trigger, fully inside the viewport)
//
// Screenshots land in end2end/.probe-screenshots/ (ignored via end2end/.gitignore).
//
// **Tuning note, recorded so nobody re-discovers this the hard way**: the
// override below sets ONLY `position-anchor: none !important` — nothing
// else. An earlier version also forced `inset: auto !important` (belt and
// suspenders, to "really" strip the anchor CSS), which backfired: CSS's
// cascade ranks *important author* declarations above *normal author*
// declarations regardless of origin, and that includes inline style — an
// element's own `style="left: …"` is a normal-author declaration, so an
// `!important` stylesheet rule for the same longhand (`inset` is shorthand
// for top/right/bottom/left) beat it outright. `getComputedStyle` after that
// version read `left: 0px` even though the inline attribute correctly said
// `left: 1018px` — the JS fallback was computing the right answer and then
// losing the cascade. `position-anchor: none` alone is sufficient: with no
// anchor associated, every `anchor()` reference and every `position-area`
// value in the per-align CSS goes invalid-at-computed-value-time and falls
// back to each property's own initial value (not `!important`), which loses
// to the JS's inline `left`/`top`/`position` normally, exactly like a real
// non-supporting engine's parser would have left those declarations inert.

import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const BASE = process.env.E2E_BASE_URL ?? "http://localhost:3000";
const email = process.env.E2E_EMAIL;
const password = process.env.E2E_PASSWORD;
if (!email || !password) {
  console.error("E2E_EMAIL / E2E_PASSWORD missing — see end2end/.env");
  process.exit(1);
}

const SHOT_DIR = new URL("./.probe-screenshots/", import.meta.url);
mkdirSync(SHOT_DIR, { recursive: true });

const FORCE_NO_ANCHOR_INIT_SCRIPT = `
  (() => {
    const realSupports = CSS.supports.bind(CSS);
    CSS.supports = (...args) => {
      const joined = args.join(" ").toLowerCase();
      if (joined.includes("position-anchor")) return false;
      return realSupports(...args);
    };
    const style = document.createElement("style");
    style.textContent = \`
      [popover] {
        position-anchor: none !important;
      }
    \`;
    // Run again on every navigation: the style element must exist before
    // the popover's own per-instance <style> block, but !important wins
    // regardless of source order, so appending once document.head exists
    // is enough.
    const attach = () => document.head && document.head.appendChild(style);
    if (document.head) attach();
    else document.addEventListener("DOMContentLoaded", attach, { once: true });
  })();
`;

let failures = 0;
function check(label, cond, detail) {
  const ok = !!cond;
  console.log(`${ok ? "PASS" : "FAIL"} — ${label}${detail ? ` (${detail})` : ""}`);
  if (!ok) failures++;
}

// Reads `locator.boundingBox()` repeatedly until two consecutive reads agree
// (within 0.5px), or a budget is exhausted. Needed because the same
// `ResizeObserver` correction #148 added for stale-height content loads also
// fires for stale-*width* content: `DestinationList`'s rows arrive async and
// can grow the panel enough to trigger an internal scrollbar, shrinking its
// rendered content width by the scrollbar's own width — the JS fallback
// re-positions correctly when that happens (`watch_panel_resize` observes
// ANY box-size change, not just height), but a single early read can sample
// a position computed for one width against a box still reporting the
// other, momentarily mid-transition. Waiting for two-in-a-row asserts the
// *settled* geometry, exactly what a human clicking the real button would
// perceive — not a looser check, just a later one.
async function stableBoundingBox(locator, { attempts = 15, interval = 150 } = {}) {
  let prev = null;
  for (let i = 0; i < attempts; i++) {
    const box = await locator.boundingBox();
    if (
      box &&
      prev &&
      Math.abs(box.x - prev.x) < 0.5 &&
      Math.abs(box.y - prev.y) < 0.5 &&
      Math.abs(box.width - prev.width) < 0.5 &&
      Math.abs(box.height - prev.height) < 0.5
    ) {
      return box;
    }
    prev = box;
    await new Promise((r) => setTimeout(r, interval));
  }
  return prev;
}

const browser = await chromium.launch();
const context = await browser.newContext();
await context.addInitScript(FORCE_NO_ANCHOR_INIT_SCRIPT);
const page = await context.newPage();
page.on("pageerror", (e) => console.error("pageerror:", String(e).slice(0, 300)));

// Sanity: confirm the override actually landed before trusting anything else.
await page.goto(`${BASE}/catalog`, { waitUntil: "networkidle" });
const supportsOverridden = await page.evaluate(() =>
  CSS.supports("position-anchor: --x"),
);
check(
  "CSS.supports('position-anchor: --x') reads false (override active)",
  supportsOverridden === false,
);

// The catalog's "Adding to" picker (`DestinationPicker`, destination.rs:347)
// only mounts `PickerBody` for an authenticated session — sign in before
// either surface, both need it anyway (the tray is `/my`-only).
await page.goto(`${BASE}/login`, { waitUntil: "networkidle" });
await page.fill("input[name=email]", email);
await page.fill("input[name=password]", password);
await page.click('button[type=submit]');
await page.waitForURL(`${BASE}/my`, { timeout: 15000 });

// ---------------------------------------------------------- catalog picker
{
  await page.goto(`${BASE}/catalog`, { waitUntil: "networkidle" });
  const trigger = page.locator('[data-name="PopoverTrigger"]', {
    hasText: "Adding to:",
  });
  await trigger.waitFor({ state: "visible", timeout: 15000 });
  const triggerBox = await trigger.boundingBox();
  await trigger.click();

  const panel = page.locator("#popover-destination-picker");
  await panel.waitFor({ state: "visible", timeout: 5000 }).catch(() => {});
  const box = await stableBoundingBox(panel);
  const viewport = page.viewportSize();

  check("catalog picker panel rendered and open", !!box);
  if (box && viewport && triggerBox) {
    check(
      "catalog picker stays fully inside the viewport",
      box.x >= 0 &&
        box.y >= 0 &&
        box.x + box.width <= viewport.width &&
        box.y + box.height <= viewport.height,
      `x=${box.x} y=${box.y} w=${box.width} h=${box.height} viewport=${viewport.width}x${viewport.height}`,
    );
    const triggerCenter = triggerBox.x + triggerBox.width / 2;
    const panelCenter = box.x + box.width / 2;
    check(
      "catalog picker centers over its trigger (Center align)",
      Math.abs(triggerCenter - panelCenter) <= 2,
      `triggerCenter=${triggerCenter} panelCenter=${panelCenter}`,
    );
  }
  await page.screenshot({
    path: new URL("catalog-adding-to.png", SHOT_DIR).pathname,
  });
  // Close so the next surface starts clean.
  await page.keyboard.press("Escape").catch(() => {});
}

// -------------------------------------------------------------- tray picker
{
  await page.goto(`${BASE}/my/all`, { waitUntil: "networkidle" });
  const rowSelect = page
    .locator('[data-testid="all-cards-row"] [data-testid="row-select"]')
    .first();
  await rowSelect.waitFor({ state: "visible", timeout: 15000 });
  await rowSelect.click();

  const moveTrigger = page.locator('[data-testid="tray-move"]');
  await moveTrigger.waitFor({ state: "visible", timeout: 10000 });
  const triggerBox = await moveTrigger.boundingBox();
  await moveTrigger.click();

  const panel = page.locator("#popover-tray-destination");
  await panel.waitFor({ state: "visible", timeout: 5000 }).catch(() => {});
  const box = await stableBoundingBox(panel);
  const viewport = page.viewportSize();

  check("tray picker panel rendered and open", !!box);
  if (box && viewport && triggerBox) {
    check(
      "tray picker stays fully inside the viewport",
      box.x >= 0 &&
        box.y >= 0 &&
        box.x + box.width <= viewport.width &&
        box.y + box.height <= viewport.height,
      `x=${box.x} y=${box.y} w=${box.width} h=${box.height} viewport=${viewport.width}x${viewport.height}`,
    );
    check(
      "tray picker opens ABOVE its bottom-docked trigger — the reported bug",
      box.y + box.height <= triggerBox.y + 1,
      `panelBottom=${box.y + box.height} triggerTop=${triggerBox.y}`,
    );
    check(
      "tray picker's trailing (right) edge aligns with the trigger's (End align)",
      Math.abs(box.x + box.width - (triggerBox.x + triggerBox.width)) <= 2,
      `panelRight=${box.x + box.width} triggerRight=${triggerBox.x + triggerBox.width}`,
    );
  }
  await page.screenshot({
    path: new URL("tray-move-to.png", SHOT_DIR).pathname,
  });
}

console.log(`\n${failures === 0 ? "ALL PASS" : `${failures} FAILURE(S)`}`);
await browser.close();
process.exit(failures === 0 ? 0 : 1);
