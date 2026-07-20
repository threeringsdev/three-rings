// One-off cleanup (not a test): report and optionally remove Lightning Bolt
// copies left in the e2e user's Inbox by *failed* runs of the quick-add spec.
//
// The spec self-cleans by undoing every `+ Have` it makes, but a mutation-
// testing run deliberately breaks that path, so a killed test can leave copies
// behind. Reads through the machine REST routes with the login fixture's
// cookies.
//
//   node cleanup-mutation-leftovers.mjs          # report only
//   node cleanup-mutation-leftovers.mjs --apply  # remove the extras
import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

const apply = process.argv.includes("--apply");
const base = "http://127.0.0.1:3000";
const stateFile = path.join(import.meta.dirname, "playwright/.auth/user.json");
if (!fs.existsSync(stateFile)) {
  console.error("no storageState — run `npx playwright test --project=setup` first");
  process.exit(2);
}

const browser = await chromium.launch();
const ctx = await browser.newContext({
  storageState: JSON.parse(fs.readFileSync(stateFile, "utf8")),
  baseURL: base,
});

const collections = await (await ctx.request.get("/api/collections")).json();
const inbox = collections.find((c) => c.is_inbox) ?? collections[0];
console.log(`inbox: ${inbox.name} (${inbox.id})`);

const view = await (
  await ctx.request.get(`/api/collections/${inbox.id}/view?limit=200`)
).json();
const bolts = view.cards.filter((c) => c.name.startsWith("Lightning Bolt"));

if (!bolts.length) {
  console.log("no Lightning Bolt holdings — nothing to clean");
} else {
  for (const b of bolts) {
    console.log(`  ${b.name} [${b.set_code} ${b.collector_number}] present=${b.present}`);
  }
  if (!apply) {
    console.log("\nreport only — re-run with --apply to remove these copies");
  } else {
    for (const b of bolts) {
      // Removal is a move to nowhere (`to = None`), the intake's inverse.
      const res = await ctx.request.post("/api/moves", {
        data: {
          from_collection_id: inbox.id,
          to_collection_id: null,
          printing_id: b.printing_id,
          quantity: b.present,
        },
      });
      console.log(`  removed ${b.present} × ${b.name} → ${res.status()}`);
    }
  }
}

await browser.close();
