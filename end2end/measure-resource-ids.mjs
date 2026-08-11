// Measurement, not a test: which serialized-resource slot does `/my`'s
// `AllCardsBody` resource read when it is created during an SPA navigation?
//
// `initial_value()` in leptos_server does not check `during_hydration` — it reads
// `__RESOLVED_RESOURCES[next monotonic id]` for *every* `Resource::new`, at any
// time. An `SsrMode::Async` page serializes its resources three times at three
// disjoint id ranges but the client consumes only the first, so the later ranges
// are unclaimed slots that a resource created after hydration can land on.
//
// Usage: node measure-resource-ids.mjs <collection-id> [drop-slot…]
// With no drop slots it reports. With slot numbers it `delete`s those slots on
// the collection page before navigating, which identifies the culprit by removal.
import { chromium } from "playwright";
import fs from "node:fs";

const [id, ...dropArgs] = process.argv.slice(2);
const drops = dropArgs.map(Number);
const COLLECTION = `/my/collections/${id}`;

const browser = await chromium.launch();
const ctx = await browser.newContext({
  storageState: JSON.parse(
    fs.readFileSync("playwright/.auth/user.json", "utf8"),
  ),
  viewport: { width: 1440, height: 900 },
  // localhost, not 127.0.0.1 — Better Auth's origin check (e2e-suite skill).
  baseURL: "http://localhost:3000",
});
const page = await ctx.newPage();

const snapshot = () =>
  page.evaluate(() => {
    const a = globalThis.__RESOLVED_RESOURCES ?? [];
    const out = {};
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== undefined) out[i] = String(a[i]).slice(0, 60);
    }
    return { len: a.length, slots: out };
  });

await page.goto(COLLECTION);
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
await page.waitForTimeout(600);

console.log("--- client __RESOLVED_RESOURCES on the collection page ---");
const before = await snapshot();
console.log("length:", before.len);
for (const [i, v] of Object.entries(before.slots)) console.log(`  [${i}] ${v}`);

if (drops.length) {
  await page.evaluate((ds) => {
    for (const d of ds) delete globalThis.__RESOLVED_RESOURCES[d];
  }, drops);
  console.log("dropped slots:", drops.join(","));
}

const requests = [];
page.on("request", (r) => {
  const u = r.url();
  if (u.includes("/api/")) requests.push(u.replace(/^https?:\/\/[^/]+/, ""));
});

// A real SPA navigation: the sidebar's own `All cards` row.
await page.locator('#sidebar-rail a[href="/my"]').first().click();
await page.waitForTimeout(2500);

console.log("\n--- after SPA nav to /my ---");
console.log("url:          ", page.url());
console.log("api requests: ", requests.length ? requests.join(", ") : "(none)");
console.log(
  "table rows:   ",
  await page.locator('[data-testid="all-cards-row"]').count(),
);
console.log(
  "empty state:  ",
  await page.locator('[data-testid="all-cards-empty"]').count(),
);

await browser.close();
