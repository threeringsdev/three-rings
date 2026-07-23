import { devices, defineConfig } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

// Load end2end/.env (gitignored): E2E_EMAIL / E2E_PASSWORD for the login
// fixture — created by seed-e2e-user.sh. Real env vars win over the file.
const envFile = path.join(__dirname, ".env");
if (fs.existsSync(envFile)) {
  for (const line of fs.readFileSync(envFile, "utf8").split("\n")) {
    const m = line.match(/^([A-Z0-9_]+)=(.*)$/);
    if (m && !(m[1] in process.env)) process.env[m[1]] = m[2];
  }
}

export default defineConfig({
  testDir: "./tests",
  timeout: 30 * 1000,
  expect: { timeout: 5000 },
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI ? "line" : [["html", { open: "never" }], ["line"]],
  use: {
    // The cargo-leptos watch/serve server (reads .env → Neon dev branch).
    baseURL: "http://127.0.0.1:3000",
    trace: "on-first-retry",
  },

  // Tiers (specs/ui-work-loop.md): the fast tier is for iterating —
  //   npx playwright test --project=chromium --grep @fast
  // The full three-browser tier runs at the end of EVERY task (revised
  // 2026-07-20; full-tier green is a precondition for [x]) — webkit is the
  // WKWebView stand-in, since desktop is untested in-loop.
  projects: [
    { name: "setup", testMatch: /auth\.setup\.ts/ },
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      dependencies: ["setup"],
    },
    {
      name: "firefox",
      use: { ...devices["Desktop Firefox"] },
      dependencies: ["setup"],
    },
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
      dependencies: ["setup"],
    },
  ],
});
