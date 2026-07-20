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
  'data-name="DialogContent"',
  'data-name="PopoverContent"',
  'data-name="SheetContent"',
  'data-name="CommandInput"',
  'data-name="CommandItem"',
  'data-name="HoverCardTrigger"',
  'data-name="Toaster"',
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
// Roving focus: the group is ONE tab stop and it follows the selection, so
// exactly one item may carry tabindex=0 and it must be the pressed one.
const gridItem = page.locator('[data-name="ToggleGroupItem"]', { hasText: 'Grid' });
if ((await listItem.getAttribute('tabindex')) !== '0' ||
    (await gridItem.getAttribute('tabindex')) !== '-1') {
  failures.push('toggle_group tabindex is not roving with the selection');
}
// label→checkbox association: clicking the label toggles the control
const cb = page.locator('#bench-rare-checkbox');
const cbState = await cb.getAttribute('data-state');
await page.locator('label[for="bench-rare-checkbox"]').click();
await page.waitForTimeout(200);
if ((await cb.getAttribute('data-state')) === cbState) {
  failures.push('label click did not toggle its checkbox (for/id association)');
}

// dialog: trigger opens (Leptos state), ESC closes (listener + cleanup path)
const dialogContent = page.locator('#bench-dialog');
await page.locator('#trigger_bench-dialog').click();
await page.waitForTimeout(250);
if ((await dialogContent.getAttribute('data-state')) !== 'open') {
  failures.push('dialog trigger did not open (data-state)');
}
await page.keyboard.press('Escape');
await page.waitForTimeout(250);
if ((await dialogContent.getAttribute('data-state')) !== 'closed') {
  failures.push('ESC did not close the dialog');
}
// programmatic open via the shared signal (the m-key path)
await page.locator('#bench-dialog-programmatic').click();
await page.waitForTimeout(250);
if ((await dialogContent.getAttribute('data-state')) !== 'open') {
  failures.push('programmatic dialog open (shared signal) failed');
}
await page.keyboard.press('Escape');
await page.waitForTimeout(250);

// popover: native Popover API opens on trigger; anchor positioning puts the
// panel adjacent to (not detached from) its trigger.
const popTrigger = page.locator('[popovertarget="popover-bench-popover"]');
await popTrigger.scrollIntoViewIfNeeded();
await popTrigger.click();
await page.waitForTimeout(300);
const popOpen = await page.evaluate(() =>
  document.getElementById('popover-bench-popover')?.matches(':popover-open'));
if (!popOpen) failures.push('popover did not reach :popover-open');
const [tb, pb] = await page.evaluate(() => {
  const t = document.querySelector('[popovertarget="popover-bench-popover"]').getBoundingClientRect();
  const p = document.getElementById('popover-bench-popover').getBoundingClientRect();
  return [
    { x: t.x, y: t.y, w: t.width, h: t.height },
    { x: p.x, y: p.y, w: p.width, h: p.height },
  ];
});
const gap = Math.min(Math.abs(tb.y - (pb.y + pb.h)), Math.abs(pb.y - (tb.y + tb.h)));
if (pb.w === 0 || gap > 60) {
  failures.push(`popover not anchor-positioned near trigger (gap ${Math.round(gap)}px)`);
}
// horizontal too: the panel must overlap or abut the trigger's x-range,
// not sit across the viewport.
const xOverlap = Math.min(tb.x + tb.w, pb.x + pb.w) - Math.max(tb.x, pb.x);
if (xOverlap < -40) {
  failures.push(`popover horizontally detached from trigger (overlap ${Math.round(xOverlap)}px)`);
}
await page.keyboard.press('Escape');
await page.waitForTimeout(200);

// sheet: opens from trigger, scroll lock engages, backdrop closes + unlocks
const sheetContent = page.locator('#bench-sheet-right');
await page.locator('#trigger_bench-sheet-right').click();
await page.waitForTimeout(350);
if ((await sheetContent.getAttribute('data-state')) !== 'open') {
  failures.push('sheet trigger did not open');
}
if ((await page.evaluate(() => document.body.style.overflow)) !== 'hidden') {
  failures.push('scroll lock did not engage while sheet open');
}
await page.locator('#bench-sheet-right_backdrop').click({ position: { x: 10, y: 10 } });
await page.waitForTimeout(700); // 300ms exit + unlock delay
if ((await sheetContent.getAttribute('data-state')) !== 'closed') {
  failures.push('sheet backdrop click did not close');
}
if ((await page.evaluate(() => document.body.style.overflow)) === 'hidden') {
  failures.push('scroll lock did not release after sheet closed');
}

// overlay stack: with sheet AND dialog open, one ESC closes only the topmost
// (the dialog, opened second); the sheet survives; a second ESC closes it.
// The dialog is opened programmatically — clicking behind the sheet's
// backdrop is (correctly) impossible, which mirrors the real stacked flow:
// a dialog launched from within an open sheet.
await page.locator('#trigger_bench-sheet-right').click();
await page.waitForTimeout(300);
await page.evaluate(() => document.getElementById('bench-dialog-programmatic').click());
await page.waitForTimeout(300);
await page.keyboard.press('Escape');
await page.waitForTimeout(300);
const dlgState = await page.locator('#bench-dialog').getAttribute('data-state');
const shtState = await sheetContent.getAttribute('data-state');
if (dlgState !== 'closed' || shtState !== 'open') {
  failures.push(`overlay stack broken: after 1 ESC dialog=${dlgState} sheet=${shtState} (want closed/open)`);
}
if ((await page.evaluate(() => document.body.style.overflow)) !== 'hidden') {
  failures.push('refcounted scroll lock released early (sheet still open)');
}
await page.keyboard.press('Escape');
await page.waitForTimeout(700);
if ((await sheetContent.getAttribute('data-state')) !== 'closed') {
  failures.push('second ESC did not close the remaining sheet');
}
if ((await page.evaluate(() => document.body.style.overflow)) === 'hidden') {
  failures.push('scroll lock stuck after all overlays closed');
}

// command: type-to-filter is reactive, ↑↓ moves the highlight, ⏎ activates.
const cmdInput = page.locator('[data-name="CommandInput"]').first();
await cmdInput.scrollIntoViewIfNeeded();
await cmdInput.fill('bind'); // matches "Trade Binder" only
await page.waitForTimeout(200);
const visibleItems = await page.locator('[data-name="CommandItem"]:visible').allTextContents();
if (!(visibleItems.length === 1 && /Binder/.test(visibleItems[0]))) {
  failures.push(`command filter wrong: visible=${JSON.stringify(visibleItems)}`);
}
await cmdInput.fill('');
await page.waitForTimeout(150);
await cmdInput.press('ArrowDown'); // highlight 2nd item
await cmdInput.press('Enter');
await page.waitForTimeout(150);
if (!(await page.textContent('body')).includes('picked: Trade Binder')) {
  failures.push('command ↑↓/⏎ keyboard selection did not pick the 2nd item');
}

// sonner: firing a toast with an undo action appears, undo dismisses it.
await page.getByRole('button', { name: 'With undo action' }).click();
await page.waitForTimeout(150);
if ((await page.locator('[data-name="Toast"]').count()) === 0) {
  failures.push('sonner toast did not appear');
}
if ((await page.locator('[data-name="Toast"] button', { hasText: 'Undo' }).count()) === 0) {
  failures.push('sonner toast missing undo action');
}
await page.locator('[data-name="Toast"] button', { hasText: 'Undo' }).click();
await page.waitForTimeout(150);
if ((await page.locator('[data-name="Toast"]').count()) !== 0) {
  failures.push('sonner undo did not dismiss the toast');
}

// hover_card: hover opens after the intent delay.
const hcTrigger = page.locator('[data-name="HoverCardTrigger"]').first();
const hcOpenNow = () => page.evaluate(() =>
  document.getElementById('hc-content-bench-hovercard')?.matches(':popover-open') ?? false);
await hcTrigger.scrollIntoViewIfNeeded();
await hcTrigger.hover();
await page.waitForTimeout(400); // > 150ms intent
if (!(await hcOpenNow())) failures.push('hover_card did not open on hover');
// trigger→content handoff: moving onto the card must NOT close it (the
// shared-timer fix — separate timers closed it ~150ms after entering content).
await page.locator('[data-name="HoverCardContent"]').hover();
await page.waitForTimeout(400);
if (!(await hcOpenNow())) failures.push('hover_card closed on trigger→content handoff');
// leaving the content closes it.
await page.mouse.move(0, 0);
await page.waitForTimeout(400);
if (await hcOpenNow()) failures.push('hover_card did not close on mouse-out');

// ID stability: two fresh SSR renders must serve identical overlay id wiring
// (deterministic caller IDs — the use_random_id class of bug).
const raw2 = await (await page.request.get(url)).text();
const idPattern = /(?:id|popovertarget)="(?:popover-)?bench-[a-z-]+"/g;
const ids1 = (raw.match(idPattern) ?? []).sort().join(',');
const ids2 = (raw2.match(idPattern) ?? []).sort().join(',');
if (!ids1 || ids1 !== ids2) {
  failures.push('overlay ids differ across SSR renders (ID stability)');
}

if (consoleIssues.length) failures.push(...consoleIssues);
console.log(`=== ${url}`);
console.log(failures.length ? failures.join('\n') : 'CLEAN — SSR markup, hydration, theme values, dark toggle all OK');
await browser.close();
process.exit(failures.length ? 1 : 0);
