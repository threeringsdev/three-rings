// Ad-hoc on-device check for the catalog quick-action surface (not a test).
//
// Scope is deliberately the **anonymous** surface: ui-work-loop's spike fixed
// the platform matrix at "dev-attach covers anonymous only" — the Tauri dev
// proxy strips POST bodies and Cookie headers, so the destination picker, the
// adds and the toasts (all session-gated) cannot be exercised here. They are
// covered by the web tiers, with webkit standing in for WKWebView. What this
// proves is that the row layout and the sign-in affordances survive the real
// Android webview.
//
// Attach first (see the android-smoke skill), then:
//   node end2end/android-quick-actions-check.mjs
import { chromium } from "playwright";

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const ctx = browser.contexts()[0];
const page = ctx.pages()[0];
const fail = [];
const ok = (label, cond, extra = "") =>
  console.log(`${cond ? "  ok  " : "  FAIL"} ${label}${extra ? ` — ${extra}` : ""}`) ||
  (cond ? null : fail.push(label));

// Never JS location.href — it destroys the execution context mid-evaluate.
await page.goto("http://tauri.localhost/catalog?q=bolt", {
  waitUntil: "networkidle",
  timeout: 30000,
});
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached", timeout: 20000 });

const results = page.locator("[data-testid=results-grid]");
ok("results grid renders", await results.isVisible());

const prompts = page.locator("[data-testid=signin-prompt]");
const promptCount = await prompts.count();
ok("anonymous quick actions render as sign-in prompts", promptCount > 0, `${promptCount} found`);

ok(
  "sign-in prompts are anchors (must work without JS)",
  promptCount > 0 && (await prompts.first().evaluate((el) => el.tagName)) === "A",
);

ok(
  "the prompt carries a ?next back to this search",
  ((await prompts.first().getAttribute("href")) ?? "").includes("next=%2Fcatalog%3Fq%3Dbolt"),
);

// The picker is session-gated; on the anonymous surface it must be absent
// rather than rendered empty or disabled.
ok(
  "no destination picker without a session",
  (await page.locator("[data-testid=destination-label]").count()) === 0,
);

// Both quick actions present per row, and tappable at a real touch size.
const box = await prompts.first().boundingBox();
ok("quick action has a tappable height", !!box && box.height >= 24, box ? `${Math.round(box.height)}px` : "no box");

ok("both Want and Have offered", promptCount % 2 === 0, `${promptCount} prompts`);

// The toaster mounts at the shell for every page, authed or not — if it threw,
// the shell would not have rendered at all, but assert the container exists so
// a regression in the mount point is caught here rather than in an authed run
// this platform can't do.
ok("toaster container mounted in the shell", (await page.locator("[data-name=Toaster]").count()) === 1);

console.log(
  fail.length ? `\nANDROID QUICK-ACTIONS CHECK FAIL: ${fail.join(", ")}` : "\nANDROID QUICK-ACTIONS CHECK PASS",
);
await browser.close();
process.exit(fail.length ? 1 : 0);
