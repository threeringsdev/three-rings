// Ad-hoc hydration-error detector for **signed-in** surfaces (not a test).
//
// The anonymous `hydration-check.mjs` cannot see a component that only renders
// for a session — the destination picker is invisible to it, so a hydration
// mismatch there would probe CLEAN. This reuses the Playwright login fixture's
// storageState (run the suite once first, or `npx playwright test --project=setup`).
//
// Usage: node hydration-check-authed.mjs <url…>
import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

const stateFile = path.join(import.meta.dirname, "playwright/.auth/user.json");
if (!fs.existsSync(stateFile)) {
  console.error(
    `no storageState at ${stateFile} — run \`npx playwright test --project=setup\` first`,
  );
  process.exit(2);
}

const urls = process.argv.slice(2);
if (!urls.length) {
  console.error("usage: node hydration-check-authed.mjs <url…>");
  process.exit(2);
}

const browser = await chromium.launch();
const ctx = await browser.newContext({
  storageState: JSON.parse(fs.readFileSync(stateFile, "utf8")),
});
let dirty = 0;
for (const url of urls) {
  const page = await ctx.newPage();
  const messages = [];
  page.on("console", (m) => {
    if (m.type() === "error" || m.type() === "warning") {
      messages.push(`${m.type()}: ${m.text().slice(0, 300)}`);
    }
  });
  page.on("pageerror", (e) =>
    messages.push(`pageerror: ${String(e).slice(0, 300)}`),
  );
  try {
    await page.goto(url, { waitUntil: "networkidle", timeout: 20000 });
    await page.waitForTimeout(1500);
  } catch (e) {
    messages.push(`nav-error: ${String(e).slice(0, 200)}`);
  }
  console.log(`\n=== ${url}`);
  console.log(
    messages.length ? messages.join("\n") : "CLEAN — no console errors/warnings",
  );
  if (messages.length) dirty++;
  await page.close();
}
await browser.close();
process.exit(dirty ? 1 : 0);
