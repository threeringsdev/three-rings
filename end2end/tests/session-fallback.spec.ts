// The idle-tab session fallback (P6-010, specs/phase-6-probes/P6-010.md).
//
// Both auth cookies live 900s, but with different fallbacks: `fetch_current_user`
// (awaited by `RequireAuth`) falls back from an expired/absent `tr_jwt` to
// `tr_session`, so the guard passes — but until this fix, `user_id_from_headers`
// (every hosted data read) had no such fallback, so the very next read in the
// same SSR pass 401ed anyway, rendering `data-failure="session"` over a fully
// live session. Deterministic and reproducible without waiting 15 minutes:
// delete `tr_jwt` from the browser context, keep `tr_session`, and do a real
// document navigation (a client-side nav would not exercise the SSR pass this
// bug lives in).
//
// This is the one honest way to simulate the 15-minute expiry the probe's own
// runtime check names: the *browser* usually drops `tr_jwt` outright once its
// matching `Max-Age=900` elapses, so "cookie present but expired" is rare in
// practice — "cookie absent" is the representative case, and it is exactly what
// `context.clearCookies` produces.

import { expect, test } from "@playwright/test";
import { AUTH_STATE } from "./helpers";

test.use({ storageState: AUTH_STATE });

test("@fast /my/all survives a stale tr_jwt with a live tr_session", async ({
  page,
}) => {
  // Sanity: the fixture actually carries both cookies before we start pulling
  // one out, so a failure below is provably about the fallback and not about a
  // fixture that never had a session cookie to fall back to.
  const before = await page.context().cookies();
  expect(before.map((c) => c.name)).toEqual(
    expect.arrayContaining(["tr_jwt", "tr_session"]),
  );

  await page.context().clearCookies({ name: "tr_jwt" });
  const afterClear = await page.context().cookies();
  expect(afterClear.some((c) => c.name === "tr_jwt")).toBe(false);
  expect(afterClear.some((c) => c.name === "tr_session")).toBe(true);

  // A full document request — a client-side navigation would reuse an
  // already-resolved guard and never exercise the SSR pass this bug lives in
  // (`RequireAuth`'s guard and the data read must both run fresh, in the same
  // pass, off the same stale request headers).
  await page.goto("/my/all");

  // (a) No session-failure arm anywhere on the page.
  await expect(page.locator('[data-failure="session"]')).toHaveCount(0);
  await expect(page.getByTestId("all-cards-error")).toHaveCount(0);

  // (b) Real collection content rendered — the fixture user's own rows, not an
  // empty or anonymous view. `all-cards.spec.ts` establishes the e2e user's
  // default `/my` view always carries at least one row.
  await expect(page.getByTestId("all-cards-table")).toBeVisible();
  await expect(page.getByTestId("all-cards-row").first()).toBeVisible();

  // (c) The response refreshed `tr_jwt` — `fetch_current_user`'s half of the
  // mechanism, unaffected by this fix but worth confirming stays true: the
  // guard's own re-mint still lands on this same response.
  const after = await page.context().cookies();
  const freshJwt = after.find((c) => c.name === "tr_jwt");
  expect(freshJwt).toBeDefined();
  expect(freshJwt?.value.length ?? 0).toBeGreaterThan(0);
});
