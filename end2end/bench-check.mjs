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

// 1. SSR: the raw response body must already carry the bench markup —
// one data-name marker per vendored component family.
const raw = await (await page.request.get(url)).text();
for (const marker of [
  'Component bench',
  'data-name="TableWrapper"',
  'id="theme"',
  'data-name="Button"',
  'data-name="Badge"',
  'data-name="Input"',
  'data-name="InputGroup"',
  'data-name="Kbd"',
  'data-name="Separator"',
  'data-name="Checkbox"',
  'data-name="Label"',
  'data-name="ToggleGroupItem"',
  'data-name="BreadcrumbList"',
  'data-name="Skeleton"',
  'data-name="Card"',
]) {
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

// The bench header toggle drives the `dark` class on <html> (session-only)
// since the app themes globally with dark as the default.
const htmlDark = () => page.evaluate(() => document.documentElement.classList.contains('dark'));
const before = await htmlDark();
await page.getByRole('button', { name: /(dark|light) mode/i }).click();
await page.waitForTimeout(300);
const after = await htmlDark();
if (before === after) {
  failures.push(`bench theme toggle broken (dark stayed ${before})`);
}
await page.getByRole('button', { name: /(dark|light) mode/i }).click(); // restore

// Interactivity spot-checks on the new vendored set.
const checkbox = page.locator('[data-name="Checkbox"]').first();
const cbBefore = await checkbox.getAttribute('data-state');
await checkbox.click();
await page.waitForTimeout(200);
if ((await checkbox.getAttribute('data-state')) === cbBefore) {
  failures.push('checkbox click did not flip data-state');
}
const listItem = page.locator('[data-name="ToggleGroupItem"]', { hasText: 'List' });
await listItem.click();
await page.waitForTimeout(200);
if (!(await page.textContent('body')).includes('mode: list')) {
  failures.push('toggle_group click did not update mode');
}
if ((await listItem.getAttribute('data-state')) !== 'on') {
  failures.push('toggle_group item did not reflect pressed state (data-state)');
}
// label→checkbox association: clicking the label toggles the control
const cb = page.locator('#bench-rare-checkbox');
const cbState = await cb.getAttribute('data-state');
await page.locator('label[for="bench-rare-checkbox"]').click();
await page.waitForTimeout(200);
if ((await cb.getAttribute('data-state')) === cbState) {
  failures.push('label click did not toggle its checkbox (for/id association)');
}

if (consoleIssues.length) failures.push(...consoleIssues);
console.log(`=== ${url}`);
console.log(failures.length ? failures.join('\n') : 'CLEAN — SSR markup, hydration, theme values, dark toggle all OK');
await browser.close();
process.exit(failures.length ? 1 : 0);
