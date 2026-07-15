// Bench drive (specs/ui-component-bench.md, verification checklist items 1-2):
// loads /dev/components, confirms SSR markup arrives before hydration, then
// that hydration works — no console errors/warnings, the theme panel's
// computed token values fill in, and the light/dark toggle flips the bench
// container's `dark` class. Usage:
//   node bench-check.mjs http://127.0.0.1:3100/dev/components
import { chromium } from 'playwright';

const url = process.argv[2] ?? 'http://127.0.0.1:3000/dev/components';
const failures = [];
const consoleIssues = [];

const browser = await chromium.launch();
const page = await browser.newPage();
page.on('console', (m) => {
  if (m.type() === 'error' || m.type() === 'warning') {
    consoleIssues.push(`${m.type()}: ${m.text().slice(0, 300)}`);
  }
});
page.on('pageerror', (e) => consoleIssues.push(`pageerror: ${String(e).slice(0, 300)}`));

// 1. SSR: the raw response body must already carry the bench markup.
const raw = await (await page.request.get(url)).text();
for (const marker of ['Component bench', 'data-name="TableWrapper"', 'id="theme"']) {
  if (!raw.includes(marker)) failures.push(`SSR body missing: ${marker}`);
}

// 2. Hydration: load, wait for wasm, then check interactivity.
await page.goto(url, { waitUntil: 'networkidle', timeout: 20000 });
await page.waitForTimeout(1500);

// The theme panel's value column is read from live computed style after
// hydration — every row still holding the SSR placeholder means the wasm
// never took over.
const values = await page.locator('#theme code:nth-child(3)').allTextContents();
if (values.length === 0) failures.push('theme panel rendered no token rows');
if (values.every((v) => v.trim() === '…')) {
  failures.push('theme values never resolved — hydration likely failed');
}

// The one dynamic control: toggling must flip `dark` on the bench container.
const container = page.locator('body > main > div').first();
const before = (await container.getAttribute('class')) ?? '';
await page.getByRole('button', { name: /dark mode/i }).click();
await page.waitForTimeout(300);
const after = (await container.getAttribute('class')) ?? '';
if (before.split(/\s+/).includes('dark') || !after.split(/\s+/).includes('dark')) {
  failures.push(`dark toggle broken (before="${before}" after="${after}")`);
}

if (consoleIssues.length) failures.push(...consoleIssues);
console.log(`=== ${url}`);
console.log(failures.length ? failures.join('\n') : 'CLEAN — SSR markup, hydration, theme values, dark toggle all OK');
await browser.close();
process.exit(failures.length ? 1 : 0);
