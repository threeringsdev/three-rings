import type { Locator, Page } from "@playwright/test";
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

/// Retry an interaction whose event handler may not be wired yet, even though
/// `hydrated(page)` already passed.
///
/// `data-hydrated` proves the *document* took over from SSR — it says nothing
/// about a specific control's own listener. Leptos hydrates a page's several
/// `<Suspense>`/`<Transition>` islands (the collection tree, the all-cards
/// table body, …) and installs global listeners (the router's click-delegate
/// on every `<a>`, the ⌘K palette's document keydown) on their own client-side
/// schedule, independent of the top-level `mark_hydrated` effect. Under
/// parallel-worker CPU pressure that gap widens enough to become visible: a
/// `.click()` or keypress lands on the DOM while it is still SSR-painted but
/// not yet wired. Playwright sees a normal, successful interaction — the
/// element was visible/enabled/stable, the key event dispatched — but no
/// app-level handler runs, so the click is silently swallowed and whatever
/// the test waits for next (a URL change, a toast, a count) never arrives.
/// Confirmed directly in this repo's own baseline runs: a `.click()` on a
/// plain `<a href>` that Playwright's own action log states was "visible,
/// enabled and stable" ran to a 30s test timeout with no navigation
/// (`responsive.spec.ts`, "a card page reached by clicking a catalog result"),
/// and ⌘K opening no dialog at all on the first press
/// (`command-palette.spec.ts`).
///
/// This is deliberately test-side, not an app-side readiness marker: the
/// swallowed sites span structurally unrelated mechanisms — a `<Suspense>`-
/// gated tree, a plain shell nav link with no async boundary at all, and a
/// global `document` keydown listener — so no single app-side stamp would
/// cover all of them without several separate, speculative touches to
/// hydration-critical code (real risk in this codebase — see the `|| ()`
/// unit-fallback trap and the tachys "unreachable" hydration panic class in
/// specs/app-ui.md). Retrying the action until its OWN declared effect is
/// observed treats the symptom directly, independent of which mechanism is
/// behind any given site, and costs nothing beyond one extra action + a short
/// poll when the first attempt already landed (the common case).
///
/// A `check` that never passes still fails loudly once `attempts` is spent —
/// this does not mask a real bug, it only absorbs the timing race
/// `hydrated()` cannot see. Not a substitute for `hydrated()`: call this
/// after it, the same as any other interaction.
export async function retryUntil(
  act: () => Promise<void>,
  check: () => Promise<boolean>,
  opts: { attempts?: number; interval?: number } = {},
): Promise<void> {
  const attempts = opts.attempts ?? 4;
  const interval = opts.interval ?? 750;
  // Bound every poll of `check` to its own short slice, race-style — found by
  // a real run (not a hypothetical): a bare `locator.textContent()` has no
  // timeout of its own in this repo's config (only `expect(...)` assertions
  // get the 5s default), so a `check` that reads a locator directly, without
  // the caller remembering to guard it with a `.count()` check first, can
  // block for the *entire remaining test budget* on one poll if that locator
  // is ever transiently absent — turning a clean "retries exhausted" failure
  // into an opaque 30s test-timeout instead. Bounding it here means a
  // caller's `check` can never do that, regardless of what it reads.
  const pollTimeout = Math.min(interval, 300);
  const bounded = () =>
    Promise.race([
      check(),
      new Promise<boolean>((resolve) => setTimeout(() => resolve(false), pollTimeout)),
    ]);
  for (let i = 0; i < attempts; i++) {
    await act();
    const deadline = Date.now() + interval;
    while (Date.now() < deadline) {
      if (await bounded()) return;
      await new Promise((r) => setTimeout(r, 50));
    }
  }
  // Every attempt is spent: fail loudly, as the doc above promises. Review
  // (P6-060, round 1) found three call sites where this helper IS the last
  // statement — a silent fall-through there turns a broken feature into a
  // green test. Sites with follow-up assertions lose nothing by this throw.
  throw new Error(
    `retryUntil: check never passed after ${attempts} attempts (interval ${interval}ms)`,
  );
}

/// A safe `check` building block for `retryUntil`/`clickUntil`: true iff
/// `locator` currently resolves to one element whose text equals `expected`.
/// The `.count()` guard is the point — a bare `locator.textContent()` waits
/// for the element to be attached rather than failing fast when it is
/// momentarily absent (e.g. between a re-render's unmount and remount),
/// which is exactly the trap `retryUntil`'s own bounding exists to catch.
/// Guarding here too reads better at the call site than repeating it inline.
export async function textEquals(locator: Locator, expected: string): Promise<boolean> {
  return (await locator.count()) === 1 && (await locator.textContent()) === expected;
}

/// `retryUntil` specialized for the common case: click a locator, retry the
/// click itself (not just the wait) until `check` observes the click's own
/// declared effect. See `retryUntil` for why this exists and why it is
/// test-side.
export async function clickUntil(
  locator: Locator,
  check: () => Promise<boolean>,
  opts: { attempts?: number; interval?: number } = {},
): Promise<void> {
  await retryUntil(() => locator.click(), check, opts);
}

// storageState written by auth.setup.ts (the login fixture). Authed tests
// opt in with `test.use({ storageState: AUTH_STATE })`.
export const AUTH_STATE = path.join(__dirname, "../playwright/.auth/user.json");
