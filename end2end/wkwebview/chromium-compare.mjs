// Chromium half of the WKWebView probe: (1) how chromium parses the
// `@position-try(flip-block)` blocks popover.rs embeds in its style strings,
// (2) the computed height of every [data-name=Command] on the real app —
// the pre/post equivalence baseline for dropping `h-full`.
import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

// end2end/.env (gitignored) — same loader playwright.config.ts uses.
const envFile = path.join(import.meta.dirname, "..", ".env");
if (fs.existsSync(envFile)) {
  for (const line of fs.readFileSync(envFile, "utf8").split("\n")) {
    const m = line.match(/^([A-Z0-9_]+)=(.*)$/);
    if (m && !(m[1] in process.env)) process.env[m[1]] = m[2];
  }
}

const BASE = process.env.BASE_URL ?? "http://localhost:3000";
const EMAIL = process.env.E2E_EMAIL;
const PASSWORD = process.env.E2E_PASSWORD;
if (!EMAIL || !PASSWORD) {
  throw new Error("E2E_EMAIL / E2E_PASSWORD missing — see end2end/.env");
}

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
const page = await ctx.newPage();

// ---- 1. parse behaviour of the app's own style strings, in chromium --------
await page.goto("about:blank");
const parse = await page.evaluate(() => {
  const STYLES_END = `right: anchor(right);
                bottom: anchor(top);
                margin-bottom: 8px;
                @position-try(flip-block) {
                top: anchor(bottom);
                bottom: auto;
                margin-top: 8px;
                margin-bottom: 0;
                }`;
  const css = `
#popover-PARSE-END {
position-anchor: --anchor-PARSE-END;
inset: auto;
${STYLES_END}
position-try-fallbacks: flip-block;
position-try-order: most-height;
position-visibility: anchors-visible;
}`;
  const s = document.createElement("style");
  s.textContent = css;
  document.head.appendChild(s);
  const rules = [...s.sheet.cssRules].map((r) => ({ type: r.constructor.name, cssText: r.cssText }));
  s.remove();
  const decls = ["position-anchor: --x", "position-area: block-start", "bottom: anchor(top)",
    "position-try-fallbacks: flip-block", "position-visibility: anchors-visible"];
  const supports = {};
  for (const d of decls) supports[d] = CSS.supports(d);
  // does chromium know `@position-try --name {}` as a real at-rule?
  const s2 = document.createElement("style");
  s2.textContent = "@position-try --probe-pt { top: anchor(bottom); }";
  document.head.appendChild(s2);
  const atRule = s2.sheet.cssRules.length ? s2.sheet.cssRules[0].constructor.name + " :: " + s2.sheet.cssRules[0].cssText : "DROPPED";
  s2.remove();
  return { ua: navigator.userAgent, supports, rules, atRule };
});
console.log("=== CHROMIUM parse ===");
console.log(JSON.stringify(parse, null, 1));

// ---- 2. every Command's used height on the real app -----------------------
await page.goto(`${BASE}/login`);
await page.fill("input[name=email]", EMAIL);
await page.fill("input[name=password]", PASSWORD);
await page.click("button[type=submit]");
await page.waitForURL("**/my", { timeout: 20000 });

await page.goto(`${BASE}/catalog?q=bolt`);
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
await page.locator('[data-testid="destination-label"]').waitFor();
await page.waitForTimeout(1500);

const snapAll = () =>
  page.evaluate(() =>
    [...document.querySelectorAll('[data-name="Command"]')].map((el) => {
      const b = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return {
        rect: { l: +b.left.toFixed(1), t: +b.top.toFixed(1), w: +b.width.toFixed(1), h: +b.height.toFixed(1) },
        offsetH: el.offsetHeight,
        height: cs.height,
        cls: String(el.className || "").slice(0, 80),
        parentPos: el.parentElement ? getComputedStyle(el.parentElement).position : null,
        parentHeight: el.parentElement ? getComputedStyle(el.parentElement).height : null,
      };
    }),
  );

const out = {};
out.railAndClosed = await snapAll();

// the catalog "Adding to" picker, open
await page.locator('[data-testid="destination-label"]').click();
await page.waitForTimeout(1200);
out.catalogPicker = await page.evaluate(() => {
  const panel = document.getElementById("popover-destination-picker");
  const btn = document.querySelector('[popovertarget="popover-destination-picker"]');
  const cmd = panel && panel.querySelector('[data-name="Command"]');
  const r = (el) => { const b = el.getBoundingClientRect(); return { l: +b.left.toFixed(1), t: +b.top.toFixed(1), r: +b.right.toFixed(1), b: +b.bottom.toFixed(1), w: +b.width.toFixed(1), h: +b.height.toFixed(1) }; };
  return {
    trigger: r(btn), panel: r(panel), panelOffsetH: panel.offsetHeight,
    command: cmd ? { rect: r(cmd), height: getComputedStyle(cmd).height } : null,
    computedPanel: { position: getComputedStyle(panel).position, positionArea: getComputedStyle(panel).positionArea,
      top: getComputedStyle(panel).top, left: getComputedStyle(panel).left,
      positionTryFallbacks: getComputedStyle(panel).positionTryFallbacks },
  };
});
await page.keyboard.press("Escape");
await page.waitForTimeout(400);

// the ⌘K palette
await page.keyboard.press("Meta+k");
await page.waitForTimeout(1200);
out.palette = await snapAll();

console.log("=== CHROMIUM app ===");
console.log(JSON.stringify(out, null, 1));

await browser.close();
