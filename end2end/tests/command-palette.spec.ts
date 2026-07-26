import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

// The global ⌘K command palette (design/command-palette.md; wireframes.pen
// frames P1/P2). The contract, in the order asserted below:
//
//   ⌘K/Ctrl+K opens from anywhere in the logged-in app and toggles · esc closes
//   · at rest it is RECENT/PLACES then COMMANDS with the first row pre-selected
//   · typing fuzzy-filters into COLLECTIONS + COMMANDS, empty groups dropping
//   out, best match pre-selected · ↑↓ walks the rows in the order they are drawn
//   · ⏎ navigates a place and runs a command · a no-match query says so instead
//   of erroring · `/` is not the palette's key · the palette does not exist for
//   an anonymous visitor, and does not exist at phone width.
//
// Two Playwright traps this file is written around (see the e2e-suite skill):
//   * a *closed* dialog keeps its box in the DOM, so `toBeVisible()` on it (or
//     on anything inside it) proves nothing — assert `data-state` on
//     `[role=dialog]`, and note the palette additionally unmounts its own
//     contents while closed, which is what the row-count assertions rely on.
//   * `{..}` spreads land a `data-testid` on the backdrop too, so every testid
//     here is scoped by `#command-palette`.
//
// This spec navigates and creates nothing except through the palette's own
// `New binder…`, which is cancelled rather than submitted — no fixture mutation.

test.use({ storageState: AUTH_STATE });

/** The dialog element itself (open state lives here, not on the backdrop). */
const dialog = (page: Page) => page.locator("#command-palette[role=dialog]");

/**
 * The rows, in document order. `[data-name=CommandItem]` is the vendored
 * marker: it is the element that carries `aria-selected`, so DOM order and
 * highlight are read off the same nodes — which is the whole point when the
 * thing under test is "does ↑↓ follow what you see".
 */
const rows = (page: Page) =>
  page.locator("#command-palette [data-name=CommandItem]");

/** Group labels, in document order (RECENT/PLACES/COLLECTIONS, COMMANDS). */
const groupLabels = (page: Page) =>
  page.locator("#command-palette [data-name=CommandGroupLabel]");

// ------------------------------------------------- fixture helpers (mutating) --
//
// The three `Undo last move` tests below write to the Neon dev branch, so each
// creates its own uniquely-named `zz-e2e-*` scratch collections and deletes them
// in a `finally` (delete cascades holdings and desires). Every database
// assertion reads `/api/cards/{id}/holdings` — the one read that does not group
// the grain away; a toast is evidence a message was raised, not that rows moved.

type Scratch = { id: string; name: string };
type Holding = { collection_id: string; quantity: number; board: string };

let scratchSeq = 0;
const scratchName = (what: string) =>
  `zz-e2e-plt-${what}-w${test.info().workerIndex}-${++scratchSeq}-` +
  Math.random().toString(36).slice(2, 7);

async function createCollection(
  request: APIRequestContext,
  kind: "binder" | "deck",
  what: string,
): Promise<Scratch> {
  const name = scratchName(what);
  const res = await request.post("/api/collections", {
    data: { parent_id: null, kind, name, format: null },
  });
  expect(res.status(), `create ${name}`).toBe(200);
  return (await res.json()) as Scratch;
}

const deleteCollection = (request: APIRequestContext, id: string) =>
  request.post(`/api/collections/${id}/delete`, { data: {} });

/// Catalog cards the signed-in user owns nowhere, so a holdings read for one of
/// them is exactly what a test wrote and nothing else.
async function unownedCards(
  request: APIRequestContext,
  n: number,
): Promise<{ oracle_id: string; printing_id: string }[]> {
  const mine = await request.get("/api/all-cards?limit=200");
  expect(mine.status(), "all cards").toBe(200);
  const taken = new Set(
    ((await mine.json()) as { cards: { card: { oracle_id: string } }[] }).cards.map(
      (r) => r.card.oracle_id,
    ),
  );
  const res = await request.get("/api/catalog/search?q=z&limit=60");
  expect(res.status(), "catalog search").toBe(200);
  const free = (
    (await res.json()) as { cards: { oracle_id: string; printing_id: string | null }[] }
  ).cards
    .filter((c) => c.printing_id && !taken.has(c.oracle_id))
    .slice(0, n);
  expect(free.length, `the fixture has fewer than ${n} unowned catalog cards`).toBe(n);
  return free as { oracle_id: string; printing_id: string }[];
}

async function addHave(
  request: APIRequestContext,
  id: string,
  printingId: string,
  quantity = 1,
) {
  const res = await request.post(`/api/collections/${id}/have`, {
    data: {
      printing_id: printingId,
      quantity,
      finish: "nonfoil",
      condition: "nm",
      language: "en",
      board: "main",
    },
  });
  expect(res.status(), "add have").toBe(200);
}

const holdingsOf = async (
  request: APIRequestContext,
  oracleId: string,
): Promise<Holding[]> =>
  (await (await request.get(`/api/cards/${oracleId}/holdings`)).json()) as Holding[];

/// Remove a whole stack the way a user does — type 0 into the stepper, commit.
async function removeStack(page: Page, printingId: string, from: number) {
  const row = page.locator(
    `[data-testid=collection-row][data-printing="${printingId}"][data-board="main"]`,
  );
  // Wait for the *stepper* to read its count, not just for `data-hydrated`: the
  // global stamp does not mean this streamed island is wired yet, and a click
  // that lands early is silently swallowed (the documented flake in the skill).
  await expect(row.locator("[data-testid=count-stepper-value]")).toHaveText(String(from));
  await row.locator("[data-testid=count-stepper-value]").click();
  await row.locator("[data-testid=count-stepper-input]").fill("0");
  await row.locator("[data-testid=count-stepper-input]").press("Enter");
}

async function openPalette(page: Page) {
  // ControlOrMeta maps to ⌘ on macOS and Ctrl elsewhere — the same split
  // `is_palette_chord` makes from `navigator.platform`.
  await page.keyboard.press("ControlOrMeta+k");
  await expect(dialog(page)).toHaveAttribute("data-state", "open");
  await expect(rows(page).first()).toBeAttached();
}

test("@fast ⌘K opens the palette at rest, toggles, and esc closes it", async ({
  page,
}) => {
  await page.goto("/my");
  await hydrated(page);

  // Positive control for every "closed" assertion below: the dialog exists in
  // the DOM the whole time, so a passing `data-state=closed` cannot be a
  // missing-element false positive.
  await expect(dialog(page)).toBeAttached();
  await expect(dialog(page)).toHaveAttribute("data-state", "closed");
  // …and while closed it holds no rows at all, so nothing behind the scrim is
  // clickable or reachable by assistive tech.
  await expect(rows(page)).toHaveCount(0);

  await openPalette(page);

  // P1: a places group, then COMMANDS, with the fixed three-command registry.
  // `allTextContents`, not `allInnerTexts` — the labels are `uppercase` in CSS,
  // so innerText would report PLACES and hide what the markup actually says.
  const labels = await groupLabels(page).allTextContents();
  expect(labels.length).toBe(2);
  // "Places" is the cold-start heading (a fresh context has no history yet);
  // "Recent" once anything has been visited. The recents test below pins that.
  expect(["Recent", "Places"]).toContain(labels[0]);
  expect(labels[1]).toBe("Commands");
  for (const label of ["New binder…", "New deck…", "Undo last move"]) {
    await expect(
      page.locator(`#command-palette [data-testid=palette-row]`, {
        hasText: label,
      }),
    ).toHaveCount(1);
  }

  // The first row is pre-selected, so ⌘K ⏎ is a two-keystroke jump.
  await expect(rows(page).first()).toHaveAttribute("aria-selected", "true");
  // The footer's keystroke ledger, verbatim from the wireframe: `↑↓ navigate ·
  // ⏎ open · esc close`. Asserted as glyph-plus-verb pairs so a footer that
  // listed the right words against the wrong keys would fail.
  const footer = page.locator("#command-palette [data-testid=palette-footer]");
  await expect(footer).toContainText("↑↓navigate");
  await expect(footer).toContainText("⏎open");
  await expect(footer).toContainText("escclose");

  // The chord toggles.
  await page.keyboard.press("ControlOrMeta+k");
  await expect(dialog(page)).toHaveAttribute("data-state", "closed");

  await openPalette(page);
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toHaveAttribute("data-state", "closed");
});

test("@fast typing ranks places and commands into groups, and ↑↓ follows the drawn order", async ({
  page,
}) => {
  await page.goto("/my");
  await hydrated(page);
  await openPalette(page);

  const atRest = await rows(page).count();
  expect(atRest).toBeGreaterThan(3);

  // `de` matches the seeded Depth* subtree, Commander Deck and Trade Binder,
  // plus two commands — so both groups survive and the fixture can tell a
  // ranked order from an unranked one.
  await page.keyboard.type("de");
  await expect(groupLabels(page).first()).toHaveText("Collections");
  await expect(groupLabels(page).nth(1)).toHaveText("Commands");
  const drawn = await rows(page).allInnerTexts();
  expect(drawn.length).toBeGreaterThan(3);
  // Best match first: a prefix beats a mid-word hit, so a Depth* row leads
  // rather than Commander Deck or Trade Binder.
  expect(drawn[0]).toContain("Depth");
  await expect(rows(page).first()).toHaveAttribute("aria-selected", "true");

  // ↑↓ walks the rows in the order they are drawn. This is the observable
  // consequence of `command`'s mount-ordered registry agreeing with the DOM;
  // see the remount test below for the mechanism.
  for (let i = 1; i < Math.min(drawn.length, 4); i += 1) {
    await page.keyboard.press("ArrowDown");
    await expect(rows(page).nth(i)).toHaveAttribute("aria-selected", "true");
  }
  await page.keyboard.press("ArrowUp");
  await expect(rows(page).nth(2)).toHaveAttribute("aria-selected", "true");

  // A group with no match drops out entirely, label included: `depth` hits no
  // command label.
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type("depth");
  await expect(groupLabels(page)).toHaveCount(1);
  await expect(groupLabels(page).first()).toHaveText("Collections");
});

test("@fast ↑↓ still follows the drawn order when COMMANDS ranks above COLLECTIONS", async ({
  page,
}) => {
  // The other half of the ordering caveat, and the one the `de` case above
  // cannot reach: `command`'s registry is filled by `CommandItem`'s component
  // *body*, which Leptos runs when the view is **constructed**, not when it is
  // inserted. So it is not enough for the palette to remount per query — the two
  // groups have to be *built* in the order they are *drawn*. An earlier version
  // built both into `let` bindings and only then chose which to put first, and
  // with a command outranking every place the registry said places-then-commands
  // while the DOM said the opposite.
  //
  // A single `n` is the trigger: `New binder…`/`New deck…` match at a prefix
  // (score 11) while `Inbox`, `Trade Binder`, `Shopping list` and
  // `Commander Deck` only match mid-word (score 1), so `commands_first` is true
  // and both groups are non-empty. Every other test in this file types either a
  // query where places win or a full phrase that leaves no place matching at all.
  await page.goto("/my");
  await hydrated(page);
  await openPalette(page);
  await page.keyboard.type("n");

  // The premise: COMMANDS really is drawn first, and COLLECTIONS really is still
  // there. Without both, everything below would pass vacuously.
  await expect(groupLabels(page).first()).toHaveText("Commands");
  await expect(groupLabels(page).nth(1)).toHaveText("Collections");
  const drawn = await rows(page).allInnerTexts();
  expect(drawn.length).toBeGreaterThan(3);
  expect(drawn[0]).toContain("New binder…");

  // The pre-selected row is the one drawn first, not the one registered first.
  await expect(rows(page).first()).toHaveAttribute("aria-selected", "true");

  // …and ↑↓ walks the drawn order across the group boundary rather than jumping
  // into the second group and clamping there.
  for (let i = 1; i < Math.min(drawn.length, 5); i += 1) {
    await page.keyboard.press("ArrowDown");
    await expect(rows(page).nth(i)).toHaveAttribute("aria-selected", "true");
  }
  await page.keyboard.press("ArrowUp");
  await expect(rows(page).nth(3)).toHaveAttribute("aria-selected", "true");
});

test("@fast every keystroke rebuilds the rows rather than reordering them", async ({
  page,
}) => {
  // The guard for `command`'s ordering caveat (app/src/components/ui/command.rs
  // → `visible_ids`): the registry is built in *mount* order, and this surface
  // ranks, so its rows only stay in registry order because the whole list is
  // torn down and rebuilt whenever it changes.
  //
  // This is not a hypothetical. The first version of the palette rendered its
  // rows from a plain `{move || …}` closure, which *looks* like a rebuild and is
  // not — tachys diffs an unkeyed collection positionally and reuses the DOM
  // nodes. This assertion caught it. Regress to that shape (or to a `<For>` keyed
  // per row, which moves nodes instead of replacing them) and it fails again.
  await page.goto("/my");
  await hydrated(page);
  await openPalette(page);
  await page.keyboard.type("d");
  await expect(rows(page).first()).toBeAttached();

  // Pin the current first row *and* an element that must survive, so a `false`
  // below cannot be "the selector found nothing".
  await page.evaluate(() => {
    const w = window as unknown as { __row?: Element | null; __field?: Element | null };
    w.__row = document.querySelector("#command-palette [data-name=CommandItem]");
    w.__field = document.querySelector("#command-palette [data-name=CommandInput]");
  });

  await page.keyboard.type("e");
  // Rows are still there — the query still matches — so this is not a vacuous
  // "it went away" pass.
  await expect(rows(page).first()).toBeAttached();

  const { rowSurvived, fieldSurvived } = await page.evaluate(() => {
    const w = window as unknown as { __row: Element; __field: Element };
    return {
      rowSurvived: document.contains(w.__row),
      fieldSurvived: document.contains(w.__field),
    };
  });
  // The positive control: `document.contains` does report survival for a node
  // that is not rebuilt.
  expect(fieldSurvived).toBe(true);
  expect(rowSurvived).toBe(false);
});

test("@fast ⏎ opens the highlighted place", async ({ page }) => {
  await page.goto("/my");
  await hydrated(page);
  await openPalette(page);

  // Type a query that pins one known seeded collection, then commit it.
  await page.keyboard.type("shoebox");
  const first = rows(page).first();
  await expect(first).toContainText("Shoebox");
  await page.keyboard.press("Enter");

  await expect(page).toHaveURL(/\/my\/collections\/[0-9a-f-]+$/);
  await expect(
    page.getByRole("heading", { level: 1 }).filter({ hasText: "Shoebox" }),
  ).toBeVisible();
  await expect(dialog(page)).toHaveAttribute("data-state", "closed");
});

test("@fast the palette carries the last-visited places, most recent first", async ({
  page,
}) => {
  await page.goto("/my");
  await hydrated(page);
  // Walk two places through the palette itself, which is also what records them.
  await openPalette(page);
  await page.keyboard.type("shoebox");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/my\/collections\//);

  await openPalette(page);
  await page.keyboard.type("shopping");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/my\/shopping$/);

  await openPalette(page);
  await expect(groupLabels(page).first()).toHaveText("Recent");
  // Shoebox is the place we came from, so it leads; the shopping list is where
  // we *are*, so it is excluded — that exclusion is what makes ⌘K ⏎ a bounce.
  await expect(rows(page).first()).toContainText("Shoebox");
  await expect(
    page.locator("#command-palette [data-palette-key=shopping]"),
  ).toHaveCount(0);
});

test("@fast a nested collection shows its parent path", async ({ page }) => {
  await page.goto("/my");
  await hydrated(page);
  await openPalette(page);
  // The seed nests Rares under Shoebox and Depth Drawer two deep.
  await page.keyboard.type("rares");
  await expect(rows(page).first()).toContainText("Shoebox");
  await expect(
    page.locator("#command-palette [data-testid=palette-meta]").first(),
  ).toHaveText("Shoebox");
});

test("@fast a no-match query says so instead of erroring", async ({ page }) => {
  await page.goto("/my");
  await hydrated(page);
  await openPalette(page);
  await page.keyboard.type("zzzznotathing");
  await expect(rows(page)).toHaveCount(0);
  await expect(
    page.locator("#command-palette [data-testid=palette-empty]"),
  ).toHaveText("No matches");
  // Still open and still typeable — typing is never an error.
  await expect(dialog(page)).toHaveAttribute("data-state", "open");
  await page.keyboard.press("Backspace");
  await expect(dialog(page)).toHaveAttribute("data-state", "open");
});

test("@fast `New binder…` opens the tree's own create dialog, from Catalog mode", async ({
  page,
}) => {
  // The command is a trigger for the tree's in-place create flow, not a second
  // one — and it has to work from Catalog mode, where the tree is not mounted.
  await page.goto("/catalog");
  await hydrated(page);
  await openPalette(page);
  await page.keyboard.type("new binder");
  await expect(rows(page).first()).toContainText("New binder…");
  await page.keyboard.press("Enter");

  await expect(page).toHaveURL(/\/my$/);
  const create = page.locator("#tree-create[role=dialog]");
  await expect(create).toHaveAttribute("data-state", "open");
  await expect(create).toContainText("New binder");
  await expect(create).toContainText("At the top level.");
  // Cancel: this spec must not mutate the fixture.
  await create.getByRole("button", { name: "Close dialog" }).first().click();
  await expect(create).toHaveAttribute("data-state", "closed");
});

test("@fast `Undo last move` reports honestly when nothing was moved", async ({
  page,
}) => {
  // There is no server-side "undo the latest" (it would race a second tab), so
  // the command replays move ids recorded in *this* session. A fresh load has
  // none and must say so rather than guessing.
  await page.goto("/my");
  await hydrated(page);
  await openPalette(page);
  await page.keyboard.type("undo");
  await expect(rows(page).first()).toContainText("Undo last move");
  await page.keyboard.press("Enter");
  await expect(page.getByText(/Nothing to undo yet/)).toBeVisible();
});

test("@fast `Undo last move` reverses the move another surface just made", async ({
  page,
  request,
}) => {
  // The happy path, and the proof that the cross-cutting `LastMoveState` wiring
  // works: the palette's command reverses a move a *different* surface (the
  // collection view's removal) recorded.
  //
  // The scratch name is deliberately *not* the command's: an earlier version
  // called it `zz-e2e-palette-undo-…`, which matched the query and exposed the
  // group-order bug now fixed in `ranked`. That bug has its own tests; this one
  // should exercise the undo rather than the ranking.
  const [card] = await unownedCards(request, 1);
  const binder = await createCollection(request, "binder", "undo");
  try {
    await addHave(request, binder.id, card.printing_id, 3);
    await page.goto(`/my/collections/${binder.id}`);
    await hydrated(page);
    await removeStack(page, card.printing_id, 3);
    await expect
      .poll(async () => (await holdingsOf(request, card.oracle_id)).length)
      .toBe(0);

    // Undo from the palette — not from the toast, which is a different button
    // wired to a different closure.
    await openPalette(page);
    await page.keyboard.type("undo");
    await expect(rows(page).first()).toContainText("Undo last move");
    await page.keyboard.press("Enter");
    await expect(page.getByText("Undid the last move")).toBeVisible();

    // The copies are back, in the collection they left, as the stack they were —
    // undo is the move ledger's reversal, not a re-add.
    const back = await holdingsOf(request, card.oracle_id);
    expect(back).toHaveLength(1);
    expect(back[0].collection_id).toBe(binder.id);
    expect(back[0].quantity).toBe(3);
  } finally {
    await deleteCollection(request, binder.id);
  }
});

test("@fast a move already undone from its toast is no longer `the last move`", async ({
  page,
  request,
}) => {
  // `LastMoveState` names the most recent *reversible* move, or nothing. Undo is
  // idempotent server-side, so replaying an already-undone id returns `Ok(())` —
  // which means a stale record makes ⌘K raise "Undid the last move" over a
  // no-op. A labelled command must not claim success for nothing.
  const [card] = await unownedCards(request, 1);
  const binder = await createCollection(request, "binder", "stale");
  try {
    await addHave(request, binder.id, card.printing_id, 2);
    await page.goto(`/my/collections/${binder.id}`);
    await hydrated(page);
    await removeStack(page, card.printing_id, 2);

    // Reverse it from the *toast*, and confirm that actually landed — this is
    // the positive control for the silence asserted below.
    const toast = page.locator('[data-name="Toast"]', { hasText: "Removed" });
    await toast.getByRole("button", { name: "Undo" }).click();
    await expect
      .poll(async () => (await holdingsOf(request, card.oracle_id)).length)
      .toBe(1);

    await openPalette(page);
    await page.keyboard.type("undo");
    await expect(rows(page).first()).toContainText("Undo last move");
    await page.keyboard.press("Enter");
    await expect(page.getByText(/Nothing to undo yet/)).toBeVisible();
    await expect(page.getByText("Undid the last move")).toHaveCount(0);

    // …and nothing moved a second time.
    const after = await holdingsOf(request, card.oracle_id);
    expect(after).toHaveLength(1);
    expect(after[0].quantity).toBe(2);
  } finally {
    await deleteCollection(request, binder.id);
  }
});

test("@fast `Undo last move` after Empty deck reverses the teardown, not an older move", async ({
  page,
  request,
}) => {
  // A teardown writes N ledger rows. Before it recorded them, ⌘K reached *past*
  // the teardown and reversed whatever unrelated move happened to be last —
  // silently moving different copies somewhere else while the teardown stood.
  // Two writes here, in order, so the test can tell those two outcomes apart.
  const [kept, emptied] = await unownedCards(request, 2);
  const binder = await createCollection(request, "binder", "older");
  const deck = await createCollection(request, "deck", "td");
  try {
    await addHave(request, binder.id, kept.printing_id, 3);
    await addHave(request, deck.id, emptied.printing_id, 2);

    // Write 1 — the older move: remove the binder's stack.
    await page.goto(`/my/collections/${binder.id}`);
    await hydrated(page);
    await removeStack(page, kept.printing_id, 3);
    await expect
      .poll(async () => (await holdingsOf(request, kept.oracle_id)).length)
      .toBe(0);

    // Write 2 — the teardown: empty the deck into the binder.
    await page.goto(`/my/collections/${deck.id}`);
    await hydrated(page);
    await page.locator('[data-testid="teardown-open"]').click();
    await page
      .locator('[data-testid="teardown-destination"]')
      .selectOption({ label: binder.name });
    await page.locator('[data-testid="teardown-confirm"]').click();
    await expect(
      page.locator('[data-name="Toast"]', { hasText: "Emptied" }),
    ).toContainText("1 card moved");
    await expect
      .poll(async () =>
        (await holdingsOf(request, emptied.oracle_id)).map((h) => h.collection_id),
      )
      .toEqual([binder.id]);

    await openPalette(page);
    await page.keyboard.type("undo");
    await page.keyboard.press("Enter");
    await expect(page.getByText("Undid the last move")).toBeVisible();

    // The teardown is what came back…
    await expect
      .poll(async () =>
        (await holdingsOf(request, emptied.oracle_id)).map((h) => h.collection_id),
      )
      .toEqual([deck.id]);
    // …and the older removal is untouched. Without this the test would pass
    // just as well against the bug, which reversed exactly this one.
    expect(await holdingsOf(request, kept.oracle_id)).toHaveLength(0);
  } finally {
    await deleteCollection(request, deck.id);
    await deleteCollection(request, binder.id);
  }
});

test("@fast `/` is not the palette's key", async ({ page }) => {
  await page.goto("/my");
  await hydrated(page);
  // Positive control first: the chord this page *does* answer.
  await openPalette(page);
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toHaveAttribute("data-state", "closed");
  // `/` belongs to the in-collection quick-add and must not reach the palette.
  await page.keyboard.press("/");
  await expect(dialog(page)).toHaveAttribute("data-state", "closed");
});

// Signed out, in its own describe so it keeps the project's `baseURL` while
// dropping the storageState the rest of the file uses.
test.describe("anonymous", () => {
  test.use({ storageState: { cookies: [], origins: [] } });

  test("@fast the palette does not exist for an anonymous visitor", async ({
    page,
  }) => {
    await page.goto("/catalog");
    await hydrated(page);
    // Positive control: the page really rendered *as an anonymous visitor*, so
    // the absence below is the auth gate and not a blank page.
    await expect(
      page.getByRole("link", { name: "Sign in" }).first(),
    ).toBeVisible();
    // Not merely closed — not mounted at all.
    await expect(page.locator("#command-palette")).toHaveCount(0);
    await page.keyboard.press("ControlOrMeta+k");
    await expect(page.locator("#command-palette")).toHaveCount(0);
  });
});

test("@fast the palette is not on mobile — the desktop gate follows the viewport", async ({
  page,
}) => {
  // The gate is a media query, so the bench's readout is where it can be
  // observed on its own (on the app pages an absent palette has two possible
  // causes). Positive control: the same readout says `true` at desktop width,
  // so a `false` at phone width is the gate acting and not a dead element.
  await page.goto("/dev/components#command-dialog");
  await hydrated(page);
  const readout = page.locator("[data-testid=bench-palette-desktop]");
  await expect(readout).toHaveText("true");

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(readout).toHaveText("false");
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(readout).toHaveText("true");
});

test("@fast the palette leaves the app when the viewport stops being desktop", async ({
  page,
}) => {
  await page.goto("/my");
  await hydrated(page);
  await openPalette(page);
  await page.setViewportSize({ width: 390, height: 844 });
  // Gone, not just closed.
  await expect(page.locator("#command-palette")).toHaveCount(0);
  // …and it comes back, which is the positive control that the count above is
  // the gate rather than a broken selector.
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(page.locator("#command-palette")).toHaveCount(1);
});
