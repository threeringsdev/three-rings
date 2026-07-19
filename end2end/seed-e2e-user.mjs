// Sign up the e2e user through the real /signup form (headless chromium).
// Lands on the OTP screen (email verification is ON — the OTP goes nowhere);
// seed-e2e-user.sh then flips emailVerified in neon_auth."user" directly.
// "User already exists" is success (idempotent re-run).
//
// Usage: node seed-e2e-user.mjs <base> <email> <password>
import { chromium } from "@playwright/test";

const [base, email, password] = process.argv.slice(2);
if (!base || !email || !password) {
  console.error("usage: node seed-e2e-user.mjs <base> <email> <password>");
  process.exit(2);
}

const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto(`${base}/signup`, { waitUntil: "networkidle" });
await page.fill("input[name=name]", "E2E Tester");
await page.fill("input[name=email]", email);
await page.fill("input[name=password]", password);
await page.click("button[type=submit]");

// Best-effort screen detection only — the DB row is the authoritative
// outcome (seed-e2e-user.sh checks it via psql). The OTP send to a
// non-deliverable address can error UI-side after the account is created.
const otp = page.locator("input[name=otp]");
const exists = page.getByText(/already exists/i);
try {
  await Promise.race([
    otp.waitFor({ timeout: 15000 }),
    exists.waitFor({ timeout: 15000 }),
  ]);
  console.log(
    (await otp.isVisible().catch(() => false))
      ? "signup: user created (OTP pending — will be flipped verified)"
      : "signup: user already exists (ok)",
  );
} catch {
  console.log(
    "signup: submitted; screen did not advance (ok if the DB row exists — the .sh verifies)",
  );
}
await browser.close();
