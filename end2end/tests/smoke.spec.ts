import { expect, test } from "@playwright/test";
import { AUTH_STATE } from "./helpers";

// Shell smoke (specs/app-ui.md "App shell"): the route map SSRs, `/`
// dispatches by auth state, the /my/* guard bounces anonymous visitors to
// /login with a return path, and the mode switch (desktop) / bottom tabs
// (mobile) navigate between Catalog and My cards.

test("catalog SSRs rendered markup @fast", async ({ request }) => {
  // request-level (no JS runs): the raw HTML must carry rendered content,
  // proving SSR rather than client-side rendering into an empty shell.
  const res = await request.get("/catalog");
  expect(res.status()).toBe(200);
  expect(await res.text()).toMatch(/<h1[^>]*>Catalog/);
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
  await page.getByRole("navigation", { name: "Mode" }).getByText("My cards").click();
  await page.waitForURL(
    (url) =>
      url.pathname === "/login" && url.searchParams.get("next") === "/my",
  );
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
});

test("login honors next after sign-in @fast", async ({ page }) => {
  // Deliberately anonymous (no storageState): drive the real login form so
  // the guard's ?next round-trip is exercised end to end.
  const email = process.env.E2E_EMAIL!;
  const password = process.env.E2E_PASSWORD!;
  await page.goto("/my/shopping");
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

  test("/ redirects the signed-in session to /my @fast", async ({ page }) => {
    await page.goto("/");
    await page.waitForURL("/my");
    await expect(page.locator("h1")).toHaveText("All cards");
  });

  test("desktop mode switch swaps Catalog and My cards @fast", async ({
    page,
  }) => {
    await page.goto("/my");
    const modeSwitch = page.getByRole("navigation", { name: "Mode" });
    await modeSwitch.getByText("Catalog").click();
    await page.waitForURL("/catalog");
    await expect(page.locator("h1")).toHaveText("Catalog");
    await modeSwitch.getByText("My cards").click();
    await page.waitForURL("/my");
    await expect(page.locator("h1")).toHaveText("All cards");
  });

  test("mobile bottom tabs replace the mode switch and navigate @fast", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/catalog");
    const tabs = page.getByRole("navigation", { name: "Primary" });
    await expect(tabs).toBeVisible();
    await expect(page.getByRole("navigation", { name: "Mode" })).toBeHidden();
    await tabs.getByText("My cards").click();
    await page.waitForURL("/my");
    await expect(page.locator("h1")).toHaveText("All cards");
    await tabs.getByText("Catalog").click();
    await page.waitForURL("/catalog");
  });

  test("user menu shows the signed-in account @fast", async ({ page }) => {
    await page.goto("/catalog");
    await page.getByRole("button", { name: "Account menu" }).click();
    await expect(page.getByText(/Signed in as/)).toBeVisible();
  });
});
