// Probe: what actually happens when + Have is clicked?
import { chromium } from "@playwright/test";
import fs from "node:fs";

const state = JSON.parse(
  fs.readFileSync(
    "/Users/dylan.goings/source/three-rings/end2end/playwright/.auth/user.json",
    "utf8",
  ),
);
const browser = await chromium.launch();
const ctx = await browser.newContext({ storageState: state });
const page = await ctx.newPage();

page.on("console", (m) => console.log(`[console.${m.type()}]`, m.text().slice(0, 300)));
page.on("pageerror", (e) => console.log("[pageerror]", String(e).slice(0, 300)));
page.on("request", (r) => {
  if (r.url().includes("/api/")) console.log("[req]", r.method(), r.url());
});
page.on("response", (r) => {
  if (r.url().includes("/api/")) console.log("[res]", r.status(), r.url());
});

await page.goto("http://127.0.0.1:3000/catalog?q=bolt");
await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
await page.locator('[data-testid=destination-label]').waitFor();
console.log("label:", await page.locator("[data-testid=destination-label]").textContent());

const have = page.getByTestId("quick-add-have").first();
console.log("have count:", await page.getByTestId("quick-add-have").count());
console.log("have disabled attr:", await have.getAttribute("disabled"));
console.log("have enabled:", await have.isEnabled());

await have.click();
await page.waitForTimeout(4000);

const toasts = await page.locator("[data-name=Toast]").allTextContents();
console.log("toasts:", JSON.stringify(toasts));

await browser.close();
