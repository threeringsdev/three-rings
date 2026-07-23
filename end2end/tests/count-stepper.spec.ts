import { expect, test, type Page } from "@playwright/test";
import { hydrated } from "./helpers";

// Count stepper — custom gap component №2 (specs/app-ui.md "Custom gap
// components"; design/component-gap-analysis.md). Exercised on its bench
// section (/dev/components is a public surface, so no auth fixture), which
// drives the same component the collection view will host.
//
// The contract, asserted below:
//   ± hidden until hover/focus · steps accumulate in one session and commit
//   ONCE on blur (not per click) · commit raises an undo toast that reverses ·
//   click-to-type swaps in the vendored Input seeded from the count · a typed
//   session commits on ⏎ AND on blur-to-external-target · ⎋ cancels · keyboard
//   ± clamps at the bounds · a failed save reverts the optimistic value.
//
// The full three-browser tier is the evidence tier: webkit stands in for
// WKWebView, where the pointerdown-focus and focusout-commit paths differ from
// Chromium (the module docs' engine note). @fast marks the core loop.

const BENCH = "/dev/components";
const basic = "#bench-stepper-basic";
const V = `${basic} [data-testid="count-stepper-value"]`;
const INC = `${basic} [data-testid="count-stepper-inc"]`;
const INPUT = `${basic} [data-testid="count-stepper-input"]`;
const LAST = '[data-testid="bench-stepper-last"]';

async function open(page: Page) {
  await page.goto(BENCH);
  await hydrated(page);
  await page.locator(V).scrollIntoViewIfNeeded();
}

// Move focus fully out of the stepper by clicking a non-interactive label,
// which blurs the active element to <body> — the realistic commit trigger.
async function blurOut(page: Page) {
  await page.locator(`${basic} span`, { hasText: "Lightning Bolt" }).click();
}

test("@fast ± are hidden at rest and revealed on hover", async ({ page }) => {
  await open(page);
  // The reveal is opacity, not display, so the buttons stay in layout — assert
  // computed opacity flips 0 → 1. (An always-visible regression fails the 0.)
  await expect
    .poll(() => page.locator(INC).evaluate((el) => getComputedStyle(el).opacity))
    .toBe("0");
  await page.locator(`${basic} [data-testid="count-stepper"]`).hover();
  await expect
    .poll(() => page.locator(INC).evaluate((el) => getComputedStyle(el).opacity))
    .toBe("1");
});

test("@fast steps accumulate and commit once on blur, with an undo toast", async ({
  page,
}) => {
  await open(page);
  await page.locator(`${basic} [data-testid="count-stepper"]`).hover();
  await page.locator(INC).click();
  await page.locator(INC).click();
  // Pending shows immediately; nothing committed yet.
  await expect(page.locator(V)).toHaveText("5");
  await expect(page.locator(LAST)).toHaveText("—");

  await blurOut(page);
  // Exactly one commit for the whole session (3 → 5), not one per click.
  await expect(page.locator(LAST)).toHaveText("3 → 5");
  const toast = page.locator('[data-name="Toast"]', { hasText: "Lightning Bolt: 3 → 5" });
  await expect(toast).toBeVisible();

  await toast.getByRole("button", { name: "Undo" }).click();
  await expect(page.locator(V)).toHaveText("3");
  await expect(page.locator(LAST)).toHaveText("5 → 3");
});

test("click-to-type swaps in the seeded Input and commits on Enter", async ({
  page,
}) => {
  await open(page);
  await page.locator(V).click();
  const input = page.locator(INPUT);
  await expect(input).toBeVisible();
  // Built on the vendored Input: the seed is a real `value` attribute (PR #43),
  // not just a bind:value property — a bare <input> would render no attribute.
  await expect(input).toHaveAttribute("value", "3");
  await expect(input).toHaveValue("3");

  await page.keyboard.type("7"); // entry selects, so typing replaces
  await page.keyboard.press("Enter");
  await expect(page.locator(V)).toHaveText("7");
  await expect(page.locator(LAST)).toHaveText("3 → 7");
});

test("a typed session commits on blur to an external target", async ({ page }) => {
  // Codex review finding: ⏎ and blur are separate commit paths; the focusout
  // path (and its webkit pointerdown-focus dependency) needs its own coverage.
  await open(page);
  await page.locator(V).click();
  await expect(page.locator(INPUT)).toBeVisible();
  await page.keyboard.type("6");
  await blurOut(page);
  await expect(page.locator(V)).toHaveText("6");
  await expect(page.locator(LAST)).toHaveText("3 → 6");
});

test("⎋ cancels a typed session without committing", async ({ page }) => {
  await open(page);
  await page.locator(V).click();
  await page.keyboard.type("9");
  await page.keyboard.press("Escape");
  // Back to the count button showing the original value; no commit fired.
  await expect(page.locator(V)).toHaveText("3");
  await expect(page.locator(LAST)).toHaveText("—");
});

test("keyboard ± on the focused count clamps at max and ⎋ clears pending", async ({
  page,
}) => {
  await open(page);
  await page.locator(V).focus();
  for (let i = 0; i < 8; i++) await page.keyboard.press("+"); // 3→…→9→clamp
  await expect(page.locator(V)).toHaveText("9");
  await page.keyboard.press("Escape"); // clears the pending session
  await expect(page.locator(V)).toHaveText("3");
  await expect(page.locator(LAST)).toHaveText("—");
});

test("the − control is inert at the min bound", async ({ page }) => {
  await open(page);
  const failing = "#bench-stepper-failing";
  const fv = `${failing} [data-testid="count-stepper-value"]`;
  const fd = `${failing} [data-testid="count-stepper-dec"]`;
  await page.locator(fv).scrollIntoViewIfNeeded();
  await page.locator(`${failing} [data-testid="count-stepper"]`).hover();
  await page.locator(fd).click(); // 2 → 1
  await page.locator(fd).click({ force: true }); // 1 → 0
  await expect(page.locator(fv)).toHaveText("0");
  // At the bound the button announces disabled and clicking it opens no
  // session (force past aria-disabled: the click must be a no-op).
  await expect(page.locator(fd)).toHaveAttribute("aria-disabled", "true");
  await page.locator(fd).click({ force: true });
  await expect(page.locator(fv)).toHaveText("0");
});

test("a failed save reverts the optimistic count", async ({ page }) => {
  await open(page);
  const failing = "#bench-stepper-failing";
  const fv = `${failing} [data-testid="count-stepper-value"]`;
  const fi = `${failing} [data-testid="count-stepper-inc"]`;
  await page.locator(fv).scrollIntoViewIfNeeded();
  await page.locator(`${failing} [data-testid="count-stepper"]`).hover();
  await page.locator(fi).click(); // 2 → 3 (optimistic)
  await page.locator(`${failing} span`, { hasText: "Failing save" }).click(); // blur → commit
  await expect(page.locator(fv)).toHaveText("3"); // optimistic value shows first
  // The pretend server rejects ~400ms later; the caller reverts + toasts.
  await expect(page.locator(fv)).toHaveText("2");
  await expect(
    page.locator('[data-name="Toast"]', { hasText: "Couldn't save count" }),
  ).toBeVisible();
});
