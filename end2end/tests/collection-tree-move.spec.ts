import { expect, test, type Page } from "@playwright/test";
import { AUTH_STATE, clickUntil, hydrated } from "./helpers";

// Tree "Move to…" — the mouse-free half of the drag layer (specs/app-ui.md
// "Collection tree"; design/information-architecture.md → "Tree management
// (create / rename / delete / move) happens in place via context menus").
//
// HTML5 drag fires on neither touch nor the keyboard, so before this the tree
// could not be reparented at all without a mouse. The contract asserted below:
//
//   a tree row is reachable by Tab and its `⋯` opens the shared menu · the menu
//   is keyboard-operable (focus lands in it, ↑↓ rove) · "Move to…" opens a
//   destination picker whose ⏎ commits a real reparent · the picker never
//   offers the moved node's own subtree (the cycle guard at the source) · "Top
//   level" un-nests · the picker's ↑↓ order is the order on screen (`command`'s
//   registration-order caveat) · and all of it works below `md`, where the rail
//   used to be `display:none` and the dialogs with it.
//
// These tests MUTATE the Neon dev branch; each creates uniquely-named scratch
// collections via the API and deletes them in a `finally` (delete cascades the
// subtree, so one delete per created root).

test.use({ storageState: AUTH_STATE });

type Summary = {
  id: string;
  parent_id: string | null;
  name: string;
  is_inbox: boolean;
  position: number;
};
type TreeRow = { summary: Summary; present: number };

let scratchSeq = 0;
function scratchName(tag: string): string {
  scratchSeq += 1;
  const w = process.env.TEST_WORKER_INDEX ?? "0";
  return `zz-e2e-${tag}-w${w}-${scratchSeq}`;
}

async function createCollection(
  page: Page,
  body: { parent_id?: string | null; kind?: "binder" | "deck"; name: string },
): Promise<Summary> {
  const resp = await page.request.post("/api/collections", {
    data: { parent_id: null, kind: "binder", format: null, ...body },
  });
  expect(resp.ok(), `create ${body.name}: ${resp.status()}`).toBeTruthy();
  return (await resp.json()) as Summary;
}

async function deleteCollection(page: Page, id: string): Promise<void> {
  await page.request.post(`/api/collections/${id}/delete`);
}

async function fetchTree(page: Page): Promise<TreeRow[]> {
  const resp = await page.request.get("/api/collection_tree");
  expect(resp.ok()).toBeTruthy();
  return ((await resp.json()) as { collections: TreeRow[] }).collections;
}

async function summaryOf(page: Page, id: string): Promise<Summary> {
  const row = (await fetchTree(page)).find((r) => r.summary.id === id);
  expect(row, `collection ${id} is gone`).toBeTruthy();
  return row!.summary;
}

// A row's own head — NOT `li > div`, which for a parent row is the
// `Collapsible` wrapper enclosing its descendants' heads too.
function rowHead(page: Page, id: string) {
  return page.locator(`[data-tree-row-head="${id}"]`);
}

const MENU = "#context-menu-tree";

function menuOpen(page: Page) {
  return page
    .locator(MENU)
    .evaluate((el: HTMLElement) => el.matches(":popover-open"));
}

// The move dialog. Paired with `[role=dialog]`: `DialogContent` renders a
// backdrop sibling that shares the panel's identifying attributes, so an
// id-only locator resolves to two elements.
function moveDialog(page: Page) {
  return page.locator('[role="dialog"]#tree-move');
}

/// Open/closed is `data-state`, never `toBeVisible()`. `DialogContent` closes
/// by fading (`data-[state=closed]:opacity-0`) and keeps its box, so a *closed*
/// dialog is "visible" to Playwright — `toBeVisible()` on it passes either way
/// and asserts nothing. (`toBeVisible()` still earns its keep for one thing:
/// whether an *ancestor* hid the whole subtree. See the mobile test.)
function expectMoveState(page: Page, state: "open" | "closed") {
  return expect(moveDialog(page)).toHaveAttribute("data-state", state);
}

// Only the rows the query left on screen. `CommandItem` hides a filtered-out
// row with `display: none` on the item while the test seam rides an inner
// span, so an unqualified locator counts every collection whatever is typed.
function moveOptions(page: Page) {
  return moveDialog(page).locator('[data-testid="destination-option"]:visible');
}

/** What currently has focus, described enough to assert on. */
function focusInfo(page: Page) {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    return {
      id: el?.id ?? "",
      role: el?.getAttribute("role") ?? "",
      text: (el?.textContent ?? "").trim(),
      rowActions: el?.getAttribute("data-tree-row-actions") ?? "",
    };
  });
}

/** Open a row's menu with the mouse — for the tests that are not about the route in. */
async function openRowMenu(page: Page, id: string) {
  await rowHead(page, id).click({ button: "right" });
  await expect.poll(() => menuOpen(page)).toBe(true);
  return page.locator(MENU);
}

async function openMovePicker(page: Page, id: string) {
  const menu = await openRowMenu(page, id);
  await menu.locator('[role="menuitem"]', { hasText: "Move to…" }).click();
  await expectMoveState(page, "open");
}

/** Strip the `📥`/`🗂` a destination row is labelled with. */
function bareName(label: string): string {
  return label.replace(/^[^\s]+\s+/, "").trim();
}

test.describe("keyboard", () => {
  test("a collection is moved without ever touching the mouse @fast", async ({
    page,
  }) => {
    const src = await createCollection(page, { name: scratchName("kb-src") });
    const dst = await createCollection(page, { name: scratchName("kb-dst") });
    try {
      await page.goto("/my");
      await hydrated(page);

      // Start where a keyboard user lands: the row's own link. Everything
      // after this is a real key press — clicking the menu item would prove
      // nothing about the path this task exists to create.
      await rowHead(page, src.id).locator("a").focus();

      // Tab reaches the row's actions button. Asserted, not assumed: if it
      // were not in the tab order the rest of this test would still "pass"
      // by opening the menu some other way.
      await page.keyboard.press("Tab");
      expect((await focusInfo(page)).rowActions).toBe(src.id);

      // ⏎ opens the shared menu *and* puts focus inside it.
      await page.keyboard.press("Enter");
      await expect.poll(() => menuOpen(page)).toBe(true);
      await expect
        .poll(async () => (await focusInfo(page)).role)
        .toBe("menuitem");
      expect((await focusInfo(page)).text).toBe("New binder inside…");

      // ↓↓ roves to "Move to…" — the third item.
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      expect((await focusInfo(page)).text).toBe("Move to…");

      await page.keyboard.press("Enter");
      await expectMoveState(page, "open");
      // The dialog focuses its own field, or the keyboard path dead-ends here.
      await expect.poll(async () => (await focusInfo(page)).id).toBe(
        "tree-move-input",
      );

      // Type to narrow, ⏎ to commit.
      await page.keyboard.type(dst.name);
      await expect(moveOptions(page)).toHaveCount(1);
      await expect(moveOptions(page)).toHaveText(new RegExp(dst.name));
      await page.keyboard.press("Enter");

      // The server moved it — and to a *defined* spot: `plan_move` lands it
      // last among its new siblings, which in an empty destination is 1.0.
      await expect
        .poll(async () => (await summaryOf(page, src.id)).parent_id)
        .toBe(dst.id);
      expect((await summaryOf(page, src.id)).position).toBe(1);
      // …and the dialog closed itself on success.
      await expectMoveState(page, "closed");
    } finally {
      await deleteCollection(page, src.id);
      await deleteCollection(page, dst.id);
    }
  });

  test("Escape leaves the menu and hands focus back to the row @fast", async ({
    page,
  }) => {
    const row = await createCollection(page, { name: scratchName("kb-esc") });
    try {
      await page.goto("/my");
      await hydrated(page);
      await rowHead(page, row.id).locator("a").focus();
      await page.keyboard.press("Tab");
      await page.keyboard.press("Enter");
      await expect.poll(() => menuOpen(page)).toBe(true);
      await expect
        .poll(async () => (await focusInfo(page)).role)
        .toBe("menuitem");

      await page.keyboard.press("Escape");
      await expect.poll(() => menuOpen(page)).toBe(false);
      // Back on the button it came from — not stranded on `<body>`, which is
      // where focus goes if nothing restores it.
      await expect
        .poll(async () => (await focusInfo(page)).rowActions)
        .toBe(row.id);
    } finally {
      await deleteCollection(page, row.id);
    }
  });
});

test.describe("the offered destinations", () => {
  test("never include the moved node's own subtree @fast", async ({ page }) => {
    const parent = await createCollection(page, { name: scratchName("cy-par") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("cy-kid"),
    });
    const other = await createCollection(page, { name: scratchName("cy-oth") });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, parent.id);

      const labels = await moveOptions(page).allInnerTexts();
      // The guard, at the source: no request the server would 409 can even be
      // asked for.
      expect(labels.some((l) => l.includes(parent.name))).toBe(false);
      expect(labels.some((l) => l.includes(child.name))).toBe(false);
      // The positive control — without it, an empty list passes the two
      // assertions above and says nothing.
      expect(labels.some((l) => l.includes(other.name))).toBe(true);
      expect(labels.some((l) => l.includes("Top level"))).toBe(true);
    } finally {
      await deleteCollection(page, parent.id);
      await deleteCollection(page, other.id);
    }
  });

  test("mark where the collection already lives, and picking it is a no-op @fast", async ({
    page,
  }) => {
    const parent = await createCollection(page, { name: scratchName("cu-par") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("cu-kid"),
    });
    const moves: string[] = [];
    page.on("request", (r) => {
      if (r.url().includes("reparent_collection")) moves.push(r.url());
    });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, child.id);

      // Exactly one row carries the ✓, and it is the current parent — the
      // count assertion is what stops "every row is ticked" from passing.
      const chosen = moveDialog(page).locator(
        '[data-testid="destination-option"][data-chosen="true"]',
      );
      await expect(chosen).toHaveCount(1);
      await expect(chosen).toHaveText(new RegExp(parent.name));

      // Picking it closes without writing anything.
      await chosen.click();
      await expectMoveState(page, "closed");
      expect((await summaryOf(page, child.id)).parent_id).toBe(parent.id);
      expect(moves, "a no-op move must not hit the API").toEqual([]);
    } finally {
      await deleteCollection(page, parent.id);
    }
  });

  test("the Inbox is offered as a destination but can never be moved @fast", async ({
    page,
  }) => {
    const inbox = (await fetchTree(page)).find((r) => r.summary.is_inbox)!;
    const src = await createCollection(page, { name: scratchName("inb-src") });
    try {
      await page.goto("/my");
      await hydrated(page);

      // The Inbox's own menu has no Move — the API refuses to reparent it, so
      // offering the action would only ever 409. (`New binder inside…` is the
      // control: the menu did open and does have items.)
      const menu = await openRowMenu(page, inbox.summary.id);
      await expect(
        menu.locator('[role="menuitem"]', { hasText: "New binder inside…" }),
      ).toBeVisible();
      await expect(
        menu.locator('[role="menuitem"]', { hasText: "Move to…" }),
      ).toHaveCount(0);
      await page.keyboard.press("Escape");
      await expect.poll(() => menuOpen(page)).toBe(false);

      // …but it *is* a legal target, exactly as it is for a drag.
      await openMovePicker(page, src.id);
      const labels = await moveOptions(page).allInnerTexts();
      expect(labels.some((l) => l.includes(inbox.summary.name))).toBe(true);
    } finally {
      await deleteCollection(page, src.id);
    }
  });
});

test.describe("committing", () => {
  test('"Top level" un-nests a nested collection @fast', async ({ page }) => {
    const parent = await createCollection(page, { name: scratchName("tl-par") });
    const child = await createCollection(page, {
      parent_id: parent.id,
      name: scratchName("tl-kid"),
    });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, child.id);
      await moveOptions(page)
        .filter({ hasText: "Top level" })
        .click();

      await expect
        .poll(async () => (await summaryOf(page, child.id)).parent_id)
        .toBe(null);
      // And the sidebar caught up (the resource refetched), so the move is
      // visible without a reload.
      await expect(
        page.locator(`li[data-tree-row="${child.id}"]`).first(),
      ).toBeVisible();
    } finally {
      await deleteCollection(page, parent.id);
      // The child is a root now, so the parent's cascade no longer takes it.
      await deleteCollection(page, child.id);
    }
  });

  test("the picker's ↓ order is the order on screen @fast", async ({ page }) => {
    // `command` builds its keyboard registry at view-*construction* time and
    // `visible_ids()` returns that order, so a consumer is only safe while
    // construction order equals document order. This picker sorts its data
    // before any row mounts and typing only hides rows — this test is what
    // pins that: whatever is drawn second must be what one ↓ selects.
    const stem = scratchName("ord");
    const src = await createCollection(page, { name: `${stem}-src` });
    const a = await createCollection(page, { name: `${stem}-a` });
    const b = await createCollection(page, { name: `${stem}-b` });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, src.id);
      // Narrow to this test's own collections (the moved node is excluded from
      // its own list, so `src` is not among them).
      await moveDialog(page).locator("#tree-move-input").fill(stem);

      const labels = await moveOptions(page).allInnerTexts();
      expect(
        labels.length,
        "need at least two rows for 'second' to mean anything",
      ).toBeGreaterThanOrEqual(2);
      const second = bareName(labels[1]);
      expect(second, "the two rows must be distinguishable").not.toBe(
        bareName(labels[0]),
      );

      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");

      const expected = (await fetchTree(page)).find(
        (r) => r.summary.name === second,
      )!;
      await expect
        .poll(async () => (await summaryOf(page, src.id)).parent_id)
        .toBe(expected.summary.id);
    } finally {
      await deleteCollection(page, src.id);
      await deleteCollection(page, a.id);
      await deleteCollection(page, b.id);
    }
  });
});

// P6-121: `plan_move`'s three `None` exits used to collapse into one
// `manage.move_open.set(false)`, whose own comment named only the "already
// there" case. A destination gone forbidden or gone entirely between the
// dialog opening and the pick hit the same silent close — the user saw the
// dialog close and had every reason to believe the move happened, when
// nothing was ever sent to the server. `commit_move` now classifies the exit
// (`MoveBlocked`, table-tested in `tree_manage.rs`'s own `#[cfg(test)]`
// module, which is the kill-verified coverage for the classification itself —
// see that module for `Forbidden`/`Gone`/`AlreadyThere`).
//
// This file's own attempt at forcing the exact `Gone` race live (delete the
// picker's own selected destination via a raw API call, then click it)
// turned out not to reach that branch at all: `move_rows`' `Suspend`
// resubscribes to the tree resource, so once any refetch lands the stale row
// simply disappears from the DOM before it can be clicked — verified directly
// against this fix while writing this test (a real, useful finding: the
// client-side `Gone`/`Forbidden` arms guard a sub-reactive-tick race that
// Leptos's own Suspense reactivity makes essentially unreachable from real
// click timing, not a gap this suite can pretend to close by asserting on
// a race it cannot win deterministically). What *is* reachable, and still the
// literal scenario this task names, is deleting the destination and picking
// it before any refetch lands at all — which sends the pick to the server and
// gets an honest rejection back. The test below pins that contract so it
// cannot regress into a silent close, and is the regression net for the
// pre-existing `(Err(e), false) => manage.error.set(...)` arm this refactor
// left untouched.
test.describe("honest exits (P6-121)", () => {
  test("a destination deleted mid-dialog surfaces an error, not a silent close @fast", async ({
    page,
  }) => {
    const src = await createCollection(page, { name: scratchName("gone-src") });
    const dst = await createCollection(page, { name: scratchName("gone-dst") });
    const moves: string[] = [];
    page.on("request", (r) => {
      if (r.url().includes("reparent_collection")) moves.push(r.url());
    });
    try {
      await page.goto("/my");
      await hydrated(page);
      await openMovePicker(page, src.id);
      const dstOption = moveOptions(page).filter({ hasText: dst.name });
      await expect(dstOption).toHaveCount(1);

      // Deleted server-side through the raw API — no client `tree.refetch()`
      // fires from this, so the picker's already-rendered row for `dst` is
      // still on screen and clickable, exactly as a user who opened the
      // dialog a moment before someone (or some other tab) deleted the
      // destination would see it.
      await deleteCollection(page, dst.id);

      // Picking it must not look like success. Before this task,
      // `commit_move`'s `plan_move(..)?` returned the same `None` this ran
      // into and `move_open.set(false)` closed the dialog on it — silently,
      // with the comment above that line naming only "already there".
      await dstOption.click();
      await expectMoveState(page, "open");
      await expect(moveDialog(page).locator("[data-tree-dialog-error]")).toHaveText(
        /not found: parent collection/,
      );
    } finally {
      await deleteCollection(page, src.id);
    }
  });

  // B(ii): `busy` is one `RwSignal<bool>` shared by all four tree dialogs
  // (`TreeManage::busy`), and `Dialog`'s ESC closes an overlay without
  // cancelling its in-flight request — dismiss a slow Delete, open Move to…,
  // and every pick used to no-op with the picker looking perfectly idle.
  // `page.route` holds the delete's response pending on cue (the same
  // deterministic pattern `collection-header-kebab.spec.ts` uses for its own
  // in-flight-request race), which is what makes this assertable without
  // gambling on real timing.
  test("the move picker renders visibly disabled while another dialog's delete is in flight @fast", async ({
    page,
  }) => {
    const busy = await createCollection(page, { name: scratchName("busy-del") });
    const src = await createCollection(page, { name: scratchName("busy-src") });
    try {
      await page.goto("/my");
      await hydrated(page);

      await page.route("**/api/delete_collection*", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 1500));
        await route.continue();
      });

      const menu = await openRowMenu(page, busy.id);
      await menu.locator('[role="menuitem"]', { hasText: "Delete…" }).click();
      await page
        .locator('[role="dialog"]', { hasText: "Delete" })
        .locator("#tree-delete-confirm")
        .click();

      // ESC closes the confirm's overlay — the request the click just fired
      // keeps running underneath it, holding `manage.busy` true.
      await page.keyboard.press("Escape");
      await expect(page.locator('[role="dialog"]#tree-delete')).toHaveAttribute(
        "data-state",
        "closed",
      );

      await openMovePicker(page, src.id);
      const list = page.locator('[data-testid="tree-move-list"]');
      await expect(list).toHaveAttribute("data-busy", "true");
      await expect(
        moveDialog(page).getByTestId("tree-move-busy"),
      ).toBeVisible();

      // Let the held response land, then the indicator clears — proving it
      // tracks `busy` live rather than being stuck permanently on.
      await page.waitForResponse(
        (r) => r.url().includes("/api/delete_collection") && r.status() === 200,
      );
      await expect(list).not.toHaveAttribute("data-busy", "true");
      await expect(
        moveDialog(page).getByTestId("tree-move-busy"),
      ).toBeHidden();
    } finally {
      await page.unroute("**/api/delete_collection*");
      await deleteCollection(page, src.id);
      // `busy` was actually deleted (the held response completed above).
    }
  });

  // The major review caught on the first pass of this markup: `move_busy`
  // read `manage.busy` without asking *whose* write set it. `commit_move`
  // sets `busy` for its own commit too (the same shared signal every
  // dialog's submit uses), so on `8b4b405` this same "another change" line
  // rendered for the span of *every ordinary successful Move* — not a rare
  // race, reproducible on every single pick. `TreeManage::move_committing`
  // is the fix: set/cleared alongside `busy` in `commit_move` and nowhere
  // else, so the foreign-busy render can tell its own write apart from
  // someone else's. This test holds the move's own `reparent_collection`
  // open (not an unrelated Delete's) and asserts the opposite of the test
  // above: no dimming, no "another change" line, for the whole span of an
  // ordinary in-flight Move.
  test("an ordinary move commit does not blame itself for 'another change' @fast", async ({
    page,
  }) => {
    const src = await createCollection(page, { name: scratchName("own-src") });
    const dst = await createCollection(page, { name: scratchName("own-dst") });
    try {
      await page.goto("/my");
      await hydrated(page);

      await page.route("**/api/reparent_collection*", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 1500));
        await route.continue();
      });

      await openMovePicker(page, src.id);
      const list = page.locator('[data-testid="tree-move-list"]');
      await moveOptions(page).filter({ hasText: dst.name }).click();

      // Still mid-flight (the route above is holding the response) — and
      // this dialog's own row list must not look like someone else's write
      // is blocking it, because nothing else is: this *is* the write.
      await expect(list).not.toHaveAttribute("data-busy", "true");
      await expect(
        moveDialog(page).getByTestId("tree-move-busy"),
      ).toBeHidden();
      await expect(page.locator("[data-tree-dialog-error]")).toHaveCount(0);

      await page.waitForResponse(
        (r) => r.url().includes("/api/reparent_collection") && r.status() === 200,
      );
      await expect
        .poll(async () => (await summaryOf(page, src.id)).parent_id)
        .toBe(dst.id);
      await expectMoveState(page, "closed");
    } finally {
      await page.unroute("**/api/reparent_collection*");
      await deleteCollection(page, src.id);
      await deleteCollection(page, dst.id);
    }
  });
});

test.describe("loading state (P6-163)", () => {
  // The double-state bug: with the tree read pending, the move dialog's own
  // `Transition` fallback ("Loading collections…") and `DestinationList`'s
  // registry-inferred `CommandEmpty` ("No collection to move into.") used to
  // render *at once* — nothing had mounted a single `command` item yet
  // either way, which a check that only counts registered items cannot tell
  // apart from "genuinely no destinations". `DestinationList`'s `loading`
  // prop, wired from the tree resource's own pending state, makes it
  // exclusive instead: `CommandEmpty` un-mounts the registry-inferred line
  // while `loading` is true, same as it already did for `failed`.
  //
  // **A `page.goto` straight to `/my/collections/:id` cannot hold this
  // pending**: `AppShell` (which calls `provide_collection_tree()`) wraps
  // every catalog/my-cards page, and a full navigation resolves that
  // `Resource` *during SSR* — its value ships already-baked into the initial
  // HTML/hydration payload, so the browser never issues its own
  // `/api/collection_tree` request for `page.route` to catch (measured: zero
  // request events on a fresh full load of `/my`). The one place a genuine
  // client-side fetch happens is the *first* client-side (SPA) navigation
  // into an `AppShell`-wrapped route this session — `/dev/components` is
  // outside that shell entirely, so landing there first and then routing in
  // client-side is what makes the fetch (and so the hold) real (measured: one
  // `request` event, on the SPA transition, not the initial load). Since
  // there is no on-page link to an arbitrary just-created scratch collection,
  // an anchor pointing at it is injected and clicked instead — a real,
  // trusted click, which is what the router's global click-delegation (same
  // mechanism `<A>` relies on) needs to intercept it as an in-app navigation
  // rather than a full reload.
  test("the move picker shows exactly one state while the tree read is pending @fast", async ({
    page,
  }) => {
    const target = await createCollection(page, {
      name: scratchName("load-tgt"),
    });
    let releaseTree: (() => void) | undefined;
    const treeHeld = new Promise<void>((resolve) => {
      releaseTree = resolve;
    });
    await page.route("**/api/collection_tree*", async (route) => {
      await treeHeld;
      await route.continue();
    });
    try {
      await page.goto("/dev/components");
      await hydrated(page);

      await page.evaluate((id) => {
        const a = document.createElement("a");
        a.href = `/my/collections/${id}`;
        a.id = "p6-163-probe-link";
        // Text content, not an empty tag: an empty inline `<a>` has a
        // zero-size box, which fails Playwright's actionability check for
        // `.click()` forever (measured: a 30s hang, not a fast failure).
        a.textContent = "p6-163 probe link";
        document.body.appendChild(a);
      }, target.id);
      // `clickUntil`, not a bare click: this anchor is injected straight into
      // the DOM rather than rendered by the app, so it is reachable before
      // the router's click-delegate has necessarily attached — the exact
      // hydration-timing race `retryUntil`'s own doc comment names (e2e-suite
      // skill).
      await clickUntil(page.locator("#p6-163-probe-link"), async () =>
        page.url().includes(target.id),
      );
      await page.waitForURL(`**/my/collections/${target.id}`);

      await page.locator('[data-testid="collection-actions"]').click();
      const menu = page.locator("#context-menu-collection-header");
      await expect
        .poll(() => menu.evaluate((el) => el.matches(":popover-open")))
        .toBe(true);
      await menu
        .locator('[role="menuitem"]')
        .filter({ hasText: "Move to…" })
        .click();
      await expectMoveState(page, "open");

      const dialog = moveDialog(page);
      // Base (pre-fix) behavior: both lines below render together. The fix
      // makes them mutually exclusive — loading shows…
      await expect(dialog.getByText("Loading collections…")).toBeVisible();
      // …the registry-inferred empty line does not, at the same time…
      await expect(
        dialog.getByText("No collection to move into."),
      ).toHaveCount(0);
      // …and nothing has resolved, so no row exists yet either.
      await expect(dialog.getByTestId("destination-option")).toHaveCount(0);

      // Let the held response land, then the loading line clears and real
      // rows take over — proving the flag tracks the resource live rather
      // than being wired to a constant.
      releaseTree?.();
      await page.waitForResponse(
        (r) => r.url().includes("/api/collection_tree") && r.status() === 200,
      );
      await expect(dialog.getByText("Loading collections…")).toHaveCount(0);
      await expect(
        dialog.locator('[data-testid="destination-option"]', {
          hasText: "Top level",
        }),
      ).toBeVisible();
    } finally {
      await page.unroute("**/api/collection_tree*");
      await deleteCollection(page, target.id);
    }
  });
});

test.describe("mobile", () => {
  // A real phone: `hasTouch` so taps are taps, and a width below `md` — the
  // width at which the rail used to be `display:none`, taking the tree, its
  // context menu and (mounted inside it) every management dialog with it.
  test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

  test("the tree, its menu and the move dialog are all reachable by touch @fast", async ({
    page,
  }) => {
    const src = await createCollection(page, { name: scratchName("mb-src") });
    const dst = await createCollection(page, { name: scratchName("mb-dst") });
    try {
      await page.goto("/my");
      await hydrated(page);

      // Closed at rest — the positive control for "visible after the tap".
      const rail = page.locator("#sidebar-rail");
      await expect(rail).toBeHidden();
      await page.getByTestId("rail-toggle").tap();
      await expect(rail).toBeVisible();

      // The row's actions button is a real tap target here (no hover to
      // reveal it on a phone).
      const actions = page.locator(`[data-tree-row-actions="${src.id}"]`);
      await expect(actions).toBeVisible();
      await actions.tap();
      await expect.poll(() => menuOpen(page)).toBe(true);

      await page.locator(MENU).locator('[role="menuitem"]', {
        hasText: "Move to…",
      }).tap();

      // The regression this pins, and the one place `toBeVisible()` on a
      // dialog says something: `data-state` would read "open" even with
      // `TreeDialogs` mounted inside the `display:none` rail, because the
      // signal flips either way. Only a zero-sized box catches the ancestor
      // having swallowed the whole subtree.
      await expectMoveState(page, "open");
      await expect(moveDialog(page)).toBeVisible();
      await moveDialog(page).locator("#tree-move-input").fill(dst.name);
      await expect(moveOptions(page)).toHaveCount(1);
      await moveOptions(page).tap();

      await expect
        .poll(async () => (await summaryOf(page, src.id)).parent_id)
        .toBe(dst.id);
    } finally {
      await deleteCollection(page, src.id);
      await deleteCollection(page, dst.id);
    }
  });
});
