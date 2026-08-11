import { expect, test } from "@playwright/test";
import { AUTH_STATE, clickUntil, hydrated } from "./helpers";

// Shell smoke (specs/app-ui.md "App shell"): the route map SSRs, `/`
// dispatches by auth state, the /my/* guard bounces anonymous visitors to
// /login with a return path, and the mode switch (desktop) / bottom tabs
// (mobile) navigate between Catalog and My cards.

test("catalog SSRs rendered markup @fast", async ({ request }) => {
  // request-level (no JS runs): the raw HTML must carry rendered content,
  // proving SSR rather than client-side rendering into an empty shell.
  const res = await request.get("/catalog");
  expect(res.status()).toBe(200);
  expect(await res.text()).toMatch(/<h1[^>]*>Catalog<\/h1>/);
});

test("anonymous / is a server-side redirect to /catalog @fast", async ({
  request,
}) => {
  const res = await request.get("/", {
    maxRedirects: 0,
    headers: { accept: "text/html" },
  });
  expect(res.status()).toBe(302);
  expect(res.headers()["location"]).toBe("/catalog");
});

test("anonymous /my bounces to login with a return path @fast", async ({
  page,
}) => {
  await page.goto("/my");
  await hydrated(page);
  await page.waitForURL(
    (url) =>
      url.pathname === "/login" && url.searchParams.get("next") === "/my",
  );
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
});

test("anonymous SPA nav to My cards bounces once to login @fast", async ({
  page,
}) => {
  // Client-side guard path (no server 302 involved): the redirect must fire
  // exactly once — a tracked location read used to compound ?next while the
  // route unmounted (next=/login%3Fnext%3D…).
  await page.goto("/catalog");
  await hydrated(page);
  // Deliberately a plain click, not clickUntil: this test's own job is to
  // police the redirect firing exactly once, so a retry-capable click would
  // fight the property under test (a second click while the first navigation
  // is still settling could produce the exact compounding this guards
  // against, as an artifact of the helper rather than a real regression).
  await page
    .getByRole("navigation", { name: "Mode" })
    .getByText("My cards")
    .click();
  await page.waitForURL(
    (url) =>
      url.pathname === "/login" && url.searchParams.get("next") === "/my",
  );
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  // Stability: the URL must not compound after settling (the regression
  // produced next=/login%3Fnext%3D… a beat later).
  await page.waitForTimeout(300);
  expect(new URL(page.url()).searchParams.get("next")).toBe("/my");
});

test("login honors next after sign-in @fast", async ({ page }) => {
  // Deliberately anonymous (no storageState): drive the real login form so
  // the guard's ?next round-trip is exercised end to end.
  const email = process.env.E2E_EMAIL!;
  const password = process.env.E2E_PASSWORD!;
  await page.goto("/my/shopping");
  await hydrated(page);
  await page.waitForURL(
    (url) =>
      url.pathname === "/login" &&
      url.searchParams.get("next") === "/my/shopping",
  );
  await page.fill("input[name=email]", email);
  await page.fill("input[name=password]", password);
  await page.click("button[type=submit]");
  await page.waitForURL("/my/shopping", { timeout: 15000 });
  await expect(page.locator("h1")).toHaveText("Shopping list");
});

test.describe("authed", () => {
  test.use({ storageState: AUTH_STATE });

  // `h1:visible`, not `h1`: `/my` carries two headings in one document — the
  // All-cards table's and the mobile root list's (app/src/my/root.rs) — with
  // exactly one of them `display`ed at any width. The visible one is both what
  // the reader gets and what a screen reader is announced, so it is the honest
  // assertion; a bare `h1` is now two elements and a strict-mode error.
  //
  // Each one is paired with `toHaveCount(1)`. On its own `h1:visible` would stop
  // failing if a *second* visible heading appeared — the exact regression the
  // bare locator used to catch through strict mode — so the count is what keeps
  // the original assertion's power.
  test("/ redirects the signed-in session to /my @fast", async ({ page }) => {
    await page.goto("/");
    await hydrated(page);
    await page.waitForURL("/my");
    await expect(page.locator("h1:visible")).toHaveText("All cards");
    await expect(page.locator("h1:visible")).toHaveCount(1);
  });

  test("desktop mode switch swaps Catalog and My cards @fast", async ({
    page,
  }) => {
    await page.goto("/my");
    await hydrated(page);
    const modeSwitch = page.getByRole("navigation", { name: "Mode" });
    await expect(modeSwitch.getByText("My cards")).toHaveAttribute(
      "aria-current",
      "page",
    );
    // clickUntil, not click + waitForURL: confirmed flaky under default
    // workers (smoke.spec.ts, e2e-suite skill "Tiers") — the Mode nav's
    // router click-delegate can still be wiring up just after `hydrated()`.
    // A same-destination re-click is harmless here (a static href, not an
    // accumulating query param), unlike the anonymous guard test above.
    await clickUntil(modeSwitch.getByText("Catalog"), async () =>
      new URL(page.url()).pathname === "/catalog",
    );
    await expect(page.locator("h1:visible")).toHaveText("Catalog");
    await expect(page.locator("h1:visible")).toHaveCount(1);
    await expect(modeSwitch.getByText("Catalog")).toHaveAttribute(
      "aria-current",
      "page",
    );
    await clickUntil(modeSwitch.getByText("My cards"), async () =>
      new URL(page.url()).pathname === "/my",
    );
    await expect(page.locator("h1:visible")).toHaveText("All cards");
    await expect(page.locator("h1:visible")).toHaveCount(1);
  });

  test("mobile bottom tabs replace the mode switch and navigate @fast", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/catalog");
    await hydrated(page);
    const tabs = page.getByRole("navigation", { name: "Primary" });
    await expect(tabs).toBeVisible();
    await expect(page.getByRole("navigation", { name: "Mode" })).toBeHidden();
    // clickUntil: same confirmed flake as the desktop mode switch above,
    // on the bottom tab bar's own copy of the nav.
    await clickUntil(tabs.getByText("My cards"), async () =>
      new URL(page.url()).pathname === "/my",
    );
    // At phone width the My-cards landing is the drill-down root list
    // (wireframes → "Mobile — My cards root"), not the All-cards table.
    await expect(page.locator("h1:visible")).toHaveText("My cards");
    await expect(page.locator("h1:visible")).toHaveCount(1);
    await clickUntil(tabs.getByText("Catalog"), async () =>
      new URL(page.url()).pathname === "/catalog",
    );
  });

  test("user menu shows the signed-in account @fast", async ({ page }) => {
    await page.goto("/catalog");
    await hydrated(page);
    await page.getByRole("button", { name: "Account menu" }).click();
    await expect(
      page.getByText(`Signed in as ${process.env.E2E_EMAIL}`),
    ).toBeVisible();
  });
});
