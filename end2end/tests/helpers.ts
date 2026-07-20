import type { Page } from "@playwright/test";
import path from "node:path";

/// Wait until the wasm client has taken over the SSR'd markup.
///
/// Every page is SSR-then-hydrate, so there is a window where the DOM is
/// present but carries no event listeners — text typed into an input during it
/// is dropped and then overwritten when hydration seeds the field from the
/// URL. Any test that types (rather than navigating to a URL) has to wait for
/// this first, or it fails intermittently under parallel load.
///
/// `data-hydrated` is stamped by an `Effect` in `app/src/lib.rs`, and Effects
/// do not run during SSR — so the attribute means hydration actually finished,
/// rather than standing in for it.
export async function hydrated(page: Page) {
  await page.locator("html[data-hydrated=true]").waitFor({ state: "attached" });
}

// storageState written by auth.setup.ts (the login fixture). Authed tests
// opt in with `test.use({ storageState: AUTH_STATE })`.
export const AUTH_STATE = path.join(__dirname, "../playwright/.auth/user.json");
