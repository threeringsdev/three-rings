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
  'data-name="CountStepper"',
  'data-name="Collapsible"',
  'data-name="Item"',
  'data-name="ItemTitle"',
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

// Re-added token variants (button Warning/Success/Bordered, badge
// Success/Warning/Info): an undefined theme token generates NO utility CSS,
// so the element silently falls back to a transparent background — assert
// each resolves a real color and the families stay distinct.
const TRANSPARENT = 'rgba(0, 0, 0, 0)';
const variantBg = {};
for (const [component, name] of [
  ['Button', 'Warning'],
  ['Button', 'Success'],
  ['Badge', 'Success'],
  ['Badge', 'Warning'],
  ['Badge', 'Info'],
]) {
  const el = page.locator(`[data-name="${component}"]`, { hasText: name }).first();
  const { bg, fg, inherited } = await el.evaluate((n) => {
    const s = getComputedStyle(n);
    return {
      bg: s.backgroundColor,
      fg: s.color,
      inherited: getComputedStyle(n.parentElement).color,
    };
  });
  if (!bg || bg === TRANSPARENT) {
    failures.push(`${component} ${name} background did not resolve (token missing?): ${bg}`);
  }
  // A missing text token drops the utility and the element just inherits —
  // the variant's own text color must differ from its container's.
  if (fg === inherited) {
    failures.push(`${component} ${name} text color did not resolve (inherited ${fg})`);
  }
  variantBg[`${component}:${name}`] = bg;
}
if (variantBg['Button:Warning'] === variantBg['Button:Success']) {
  failures.push('button Warning and Success resolved to the same background');
}
const badgeBgs = new Set([
  variantBg['Badge:Success'],
  variantBg['Badge:Warning'],
  variantBg['Badge:Info'],
]);
if (badgeBgs.size !== 3) {
  failures.push('badge Success/Warning/Info backgrounds are not distinct');
}
// Bordered is transparent by design; its border must carry the token color.
// A missing border token falls back to currentcolor, so equality with the
// text color is the failure signature — not just transparency.
const bordered = page.locator('[data-name="Button"]', { hasText: 'Bordered' }).first();
const border = await bordered.evaluate((n) => {
  const s = getComputedStyle(n);
  return { width: s.borderTopWidth, color: s.borderTopColor, text: s.color };
});
if (border.width === '0px' || border.color === TRANSPARENT || border.color === border.text) {
  failures.push(`button Bordered border did not resolve: ${JSON.stringify(border)}`);
}

// collapsible: trigger flips data-state + aria-expanded, and closed content
// is inert (the grid animation keeps it in the DOM — its links must not be
// tab-reachable).
const colOpen = page.locator('#bench-collapsible-open');
const colOpenTrigger = page.locator('[aria-controls="bench-collapsible-open"]');
if ((await colOpen.getAttribute('data-state')) !== 'open') {
  failures.push('collapsible default_open did not render open');
}
await colOpenTrigger.click();
await page.waitForTimeout(200);
if ((await colOpen.getAttribute('data-state')) !== 'closed') {
  failures.push('collapsible trigger click did not close it');
}
if ((await colOpenTrigger.getAttribute('aria-expanded')) !== 'false') {
  failures.push('collapsible aria-expanded did not follow the close');
}
await colOpenTrigger.click(); // restore
await page.waitForTimeout(200);
if (!(await page.locator('#bench-collapsible-closed').evaluate((el) => el.inert))) {
  failures.push('closed collapsible content is not inert');
}

// item: the href arm must render a real <a> (the tree's pinned rows).
if ((await page.locator('a[data-name="Item"]').count()) === 0) {
  failures.push('item href arm did not render an <a>');
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

// count_stepper (custom gap component): hover-reveal ±, one commit + one undo
// toast per blur-ended session, click-to-type on the vendored Input, ⎋ cancel
// in both modes, min/max clamping, and the failing-save revert contract.
const stepBasic = page.locator('#bench-stepper-basic [data-testid="count-stepper"]');
const stepValue = page.locator('#bench-stepper-basic [data-testid="count-stepper-value"]');
const stepInc = page.locator('#bench-stepper-basic [data-testid="count-stepper-inc"]');
const stepLast = page.locator('[data-testid="bench-stepper-last"]');
const blurActive = () => page.evaluate(() => {
  if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
});
await stepBasic.scrollIntoViewIfNeeded();
await page.waitForTimeout(250);
const restOpacity = await stepInc.evaluate((el) => getComputedStyle(el).opacity);
await stepBasic.hover();
await page.waitForTimeout(300); // transition-opacity
const hoverOpacity = await stepInc.evaluate((el) => getComputedStyle(el).opacity);
if (!(parseFloat(restOpacity) === 0 && parseFloat(hoverOpacity) === 1)) {
  failures.push(`stepper ± not hover-revealed (rest ${restOpacity}, hover ${hoverOpacity})`);
}
// two + clicks show pending immediately but commit nothing until blur
await stepInc.click();
await stepInc.click();
await page.waitForTimeout(150);
if ((await stepValue.textContent())?.trim() !== '5') {
  failures.push(`stepper ++ did not show pending 5 (got ${await stepValue.textContent()})`);
}
if ((await stepLast.textContent()) !== '—') {
  failures.push('stepper committed before blur (session should be open)');
}
await blurActive();
await page.waitForTimeout(200);
if ((await stepLast.textContent()) !== '3 → 5') {
  failures.push(`stepper blur did not commit 3 → 5 (got ${await stepLast.textContent()})`);
}
// Commit cardinality: a two-click session must fire on_commit exactly ONCE
// (a lone `last` caption is overwritten by a duplicate and can't prove this).
const stepCount = page.locator('[data-testid="bench-stepper-count"]');
if ((await stepCount.textContent()) !== '1') {
  failures.push(`stepper committed ${await stepCount.textContent()} times, want exactly 1`);
}
const stepToast = page.locator('[data-name="Toast"]', { hasText: 'Lightning Bolt: 3 → 5' });
if ((await stepToast.count()) === 0) {
  failures.push('stepper commit raised no undo toast');
} else {
  await stepToast.locator('button', { hasText: 'Undo' }).click();
  await page.waitForTimeout(200);
  if ((await stepValue.textContent())?.trim() !== '3') {
    failures.push('stepper undo did not restore the old count');
  }
  if ((await stepLast.textContent()) !== '5 → 3') {
    failures.push('stepper undo did not re-commit through on_commit');
  }
}
// click-to-type: the count swaps for the vendored Input, seeded + selected
await stepValue.click();
await page.waitForTimeout(200);
const stepInput = page.locator('#bench-stepper-basic [data-testid="count-stepper-input"]');
if ((await stepInput.count()) === 0) {
  failures.push('stepper click-to-type mounted no input');
} else {
  if ((await stepInput.inputValue()) !== '3') {
    failures.push(`stepper input not seeded with the count (got ${await stepInput.inputValue()})`);
  }
  // The seed must be the vendored Input's `value` *attribute* path (PR #43),
  // not merely a bind:value property write — a bare <input bind:value> would
  // set the property but render no attribute. Asserting the attribute is what
  // distinguishes "built on Input" from a raw element that re-inherits the
  // SSR-empty trap the spec calls out.
  if ((await stepInput.getAttribute('value')) !== '3') {
    failures.push(`stepper input missing the seeded value attribute (got ${await stepInput.getAttribute('value')})`);
  }
  await page.keyboard.type('7'); // select-all on entry: typing replaces
  await page.keyboard.press('Enter');
  await page.waitForTimeout(200);
  if ((await stepValue.textContent())?.trim() !== '7') {
    failures.push('stepper ⏎ did not commit the typed count');
  }
  if ((await stepLast.textContent()) !== '3 → 7') {
    failures.push(`stepper typed commit event wrong (got ${await stepLast.textContent()})`);
  }
}
// commit a typed session by BLURRING to an external target (not ⏎): the
// focusout path must commit, not just the Enter path.
await stepValue.click();
await page.waitForTimeout(200);
await page.keyboard.type('4');
await blurActive();
await page.waitForTimeout(200);
if ((await stepValue.textContent())?.trim() !== '4') {
  failures.push(`stepper blur from edit mode did not commit (got ${await stepValue.textContent()})`);
}
if ((await stepLast.textContent()) !== '7 → 4') {
  failures.push(`stepper edit-blur commit event wrong (got ${await stepLast.textContent()})`);
}
// ⎋ cancels the typed session without committing (value stays at 4)
await stepValue.click();
await page.waitForTimeout(200);
await page.keyboard.type('9');
await page.keyboard.press('Escape');
await page.waitForTimeout(200);
if ((await stepValue.textContent())?.trim() !== '4') {
  failures.push(`stepper ⎋ did not cancel the typed session (got ${await stepValue.textContent()})`);
}
if ((await stepLast.textContent()) !== '7 → 4') {
  failures.push('stepper ⎋ still committed (last-commit changed)');
}
// keyboard ± on the focused count, clamped at max=9; ⎋ clears pending steps
await stepValue.evaluate((el) => el.focus());
for (let i = 0; i < 6; i++) await page.keyboard.press('+'); // 4→…→9→clamp
await page.waitForTimeout(150);
if ((await stepValue.textContent())?.trim() !== '9') {
  failures.push(`stepper keyboard + did not step/clamp to max (got ${await stepValue.textContent()})`);
}
await page.keyboard.press('Escape');
await page.waitForTimeout(150);
if ((await stepValue.textContent())?.trim() !== '4') {
  failures.push('stepper display-mode ⎋ did not clear pending steps');
}
// failing save: min-clamped session commits optimistically, then the caller
// reverts (the pretend server rejects 400ms later) and reports the error.
const failStep = page.locator('#bench-stepper-failing [data-testid="count-stepper"]');
const failValue = page.locator('#bench-stepper-failing [data-testid="count-stepper-value"]');
const failDec = page.locator('#bench-stepper-failing [data-testid="count-stepper-dec"]');
await failStep.scrollIntoViewIfNeeded();
await failStep.hover();
await failDec.click();
await failDec.click(); // 2 → 1 → 0
await page.waitForTimeout(150);
// at the min bound the − button reports itself disabled (Playwright refuses
// to click aria-disabled elements, which is itself the signal working) —
// keyboard − must clamp rather than go negative
if ((await failDec.getAttribute('aria-disabled')) !== 'true') {
  failures.push('stepper − not aria-disabled at min');
}
await page.keyboard.press('-');
await page.waitForTimeout(150);
if ((await failValue.textContent())?.trim() !== '0') {
  failures.push(`stepper − did not clamp at min 0 (got ${await failValue.textContent()})`);
}
await blurActive();
// Optimistic-first: the commit applies value.set(to) synchronously, so the
// count shows 0 the instant the session closes — BEFORE the pretend server
// rejects ~400ms later. Checking only the eventual 2 would pass even if the
// optimistic write were skipped (the pending clear falls back to value=2),
// so this early assertion is the one that proves the optimistic path (Codex
// mutation pass item 17).
await page.waitForTimeout(120);
if ((await failValue.textContent())?.trim() !== '0') {
  failures.push(`failing save did not apply the optimistic 0 (got ${await failValue.textContent()})`);
}
await page.waitForTimeout(600); // let the simulated 400ms rejection land
if ((await failValue.textContent())?.trim() !== '2') {
  failures.push('failing save did not revert the optimistic count');
}
if ((await page.locator('[data-name="Toast"]', { hasText: "Couldn't save count" }).count()) === 0) {
  failures.push('failing save raised no error toast');
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
await page.locator('#hc-content-bench-hovercard').hover();
await page.waitForTimeout(400);
if (!(await hcOpenNow())) failures.push('hover_card closed on trigger→content handoff');
// leaving the content closes it.
await page.mouse.move(0, 0);
await page.waitForTimeout(400);
if (await hcOpenNow()) failures.push('hover_card did not close on mouse-out');

// hover_card `disabled`: suppressed opens, and disabling closes an open card.
const hcDisTrigger = page.locator('#bench-hovercard-disabled-anchor');
const hcDisOpen = () => page.evaluate(() =>
  document.getElementById('hc-content-bench-hovercard-disabled')?.matches(':popover-open') ?? false);
const disableBtn = page.locator('[data-testid="bench-hovercard-disable"]');
await hcDisTrigger.scrollIntoViewIfNeeded();
// enabled: opens as normal
await hcDisTrigger.hover();
await page.waitForTimeout(400);
if (!(await hcDisOpen())) failures.push('hover_card(disabled=false) did not open');
// disabling while open must take it down
await disableBtn.click();
await page.waitForTimeout(300);
if (await hcDisOpen()) failures.push('hover_card did not close when disabled');
// and it must stay shut on a fresh hover
await page.mouse.move(0, 0);
await page.waitForTimeout(200);
await hcDisTrigger.hover();
await page.waitForTimeout(400);
if (await hcDisOpen()) failures.push('disabled hover_card opened on hover');

// context_menu: right-click opens the manual popover at the pointer (the
// auto-popover light-dismiss race is why it is manual), an item runs its
// action and closes, ESC and outside-click both close.
const ctxTarget = page.locator('[data-bench-context-target]');
const ctxMenu = page.locator('#context-menu-bench-context-menu');
const ctxOpen = () => ctxMenu.evaluate((el) => el.matches(':popover-open'));
await ctxTarget.scrollIntoViewIfNeeded();
await ctxTarget.click({ button: 'right' });
await page.waitForTimeout(150);
if (!(await ctxOpen())) failures.push('context_menu did not open on right-click');
await page.locator('#context-menu-bench-context-menu [role="menuitem"]', { hasText: 'Rename…' }).click();
await page.waitForTimeout(150);
if (await ctxOpen()) failures.push('context_menu did not close after an item click');
if ((await page.locator('[data-bench-context-last]').textContent()) !== 'rename') {
  failures.push('context_menu item did not run its on_select');
}
await ctxTarget.click({ button: 'right' });
await page.waitForTimeout(150);
await page.keyboard.press('Escape');
await page.waitForTimeout(150);
if (await ctxOpen()) failures.push('context_menu did not close on ESC');
// outside-click dismiss (the custom pointerdown light-dismiss — distinct code
// path from ESC; an empty `pointer_outside` would survive an ESC-only check).
await ctxTarget.click({ button: 'right' });
await page.waitForTimeout(150);
await page.mouse.click(3, 3);
await page.waitForTimeout(150);
if (await ctxOpen()) failures.push('context_menu did not close on outside click');

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
