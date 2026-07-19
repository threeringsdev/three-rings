import { expect, test } from "@playwright/test";
import { AUTH_STATE } from "./helpers";

// Baseline smoke: SSR serves, and the login fixture yields a signed-in
// session. The shell task rewrites this when the counter dies.

test("home page SSRs rendered markup @fast", async ({ request }) => {
  // request-level (no JS runs): the raw HTML must carry rendered content,
  // proving SSR rather than client-side rendering into an empty shell.
  const res = await request.get("/");
  expect(res.status()).toBe(200);
  expect(await res.text()).toMatch(/<h1[^>]*>[^<]/);
});

test("home page renders in-browser @fast", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("h1").first()).toBeVisible();
});

test.describe("authed", () => {
  test.use({ storageState: AUTH_STATE });

  test("login fixture restores a signed-in session @fast", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText(/Signed in as/)).toBeVisible();
  });
});
