import { readFileSync } from "node:fs";

import { expect, test as setup } from "@playwright/test";
import { AUTH_STATE } from "./helpers";

// Signs in once as the pre-seeded verified e2e user and saves storageState
// (captures the tr_session/tr_jwt httpOnly cookies). Authed tests opt in with
//   test.use({ storageState: AUTH_STATE })
// Never sign up in a test: email verification is ON and the OTP goes nowhere
// (specs/ui-work-loop.md; the e2e-suite skill).
// @fast so `--grep @fast` doesn't filter the setup dependency away.
setup("authenticate as the e2e user @fast", async ({ page }) => {
  const email = process.env.E2E_EMAIL;
  const password = process.env.E2E_PASSWORD;
  if (!email || !password) {
    throw new Error(
      "E2E_EMAIL / E2E_PASSWORD missing — run end2end/seed-e2e-user.sh once " +
        "to create the verified test user and end2end/.env",
    );
  }
  await page.goto("/login");
  await page.fill("input[name=email]", email);
  await page.fill("input[name=password]", password);
  await page.click("button[type=submit]");
  // Sign-in navigates to / whose redirect sends authed sessions to /my.
  await page.waitForURL("/my", { timeout: 15000 });
  await page.click('button[aria-label="Account menu"]');
  await expect(page.getByText(`Signed in as ${email}`)).toBeVisible({
    timeout: 10000,
  });
  await page.context().storageState({ path: AUTH_STATE });
  // The fixture's whole point is capturing the httpOnly session cookies —
  // fail loudly here rather than mysteriously in every authed test.
  const { cookies } = JSON.parse(readFileSync(AUTH_STATE, "utf8"));
  const names = cookies.map((c: { name: string }) => c.name);
  expect(names).toContain("tr_session");
  expect(names).toContain("tr_jwt");
});
