// `/my/collections/:id` — the binder / deck view (specs/app-ui.md).
//
// The load-bearing contracts, in assertion order:
//
// - the page SSRs (rows, header counts and the deck header in the raw HTML) —
//   the route is `SsrMode::Async` precisely so this holds;
// - the three numeric columns say what the collection read says, under the
//   spec's display rules (WANTED only when set and different, OWNED collapses
//   when it equals what's here, rolled-up counts italic + dimmed);
// - a card the collection only *wants* is still a row — the same FULL OUTER
//   JOIN correction `/my` needed, and the reason a deck's needs are visible at
//   all;
// - child collections are folder rows above the cards, carrying the rolled-up
//   count the sidebar badge shows;
// - the header counts the whole collection, not the visible page, and the needs
//   chip agrees with it;
// - the deck variant: format + commander header, type/board sections with slot
//   counts, and the teardown flow;
// - HERE is editable in place and the header follows the edit;
// - quick search is URL-canonical and filters *this* collection.
//
// **Isolation.** Every test that writes does so inside `zz-e2e-…` collections
// it creates via the API and deletes in a `finally` (the convention
// `collection-tree-manage.spec.ts` established) — so the stepper and teardown
// cases never touch the seeded tree that this file's own read assertions, and
// `all-cards.spec.ts`, are checking at the same time. Delete cascades holdings,
// so a scratch copy leaves nothing behind.
//
// The read assertions still cross-check against the API inside `toPass`, for
// the reason `all-cards.spec.ts` documents: other specs write to the same dev
// user in parallel workers.

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { AUTH_STATE, hydrated } from "./helpers";

test.use({ storageState: AUTH_STATE });

const quick = expect.configure({ timeout: 2000 });

/// HERE-cell-scoped stepper locators. Every collection row carries **two**
/// steppers since WANTED became a still-needed count you can edit
/// (specs/app-ui.md, maintainer ruling 2026-08-19), so a row-scoped
/// `count-stepper-*` locator resolves to two elements and fails Playwright's
/// strict mode. Say which column you mean.
const HERE_VALUE =
  '[data-testid="here-cell"] [data-testid="count-stepper-value"]';
const HERE_INC = '[data-testid="here-cell"] [data-testid="count-stepper-inc"]';
const HERE_DEC = '[data-testid="here-cell"] [data-testid="count-stepper-dec"]';

type Summary = {
  id: string;
  parent_id: string | null;
  kind: "binder" | "deck";
  name: string;
  is_inbox: boolean;
  format: string | null;
};
type Row = {
  oracle_id: string;
  printing_id: string;
  name: string;
  type_line: string | null;
  present: number;
  /// The card's copies here across every printing on this board — folded
  /// server-side, because the rows are one keyset page (see CardRow's doc).
  present_group: number;
  desired: number;
  owned: number;
  present_rollup: number;
  board: "main" | "side" | "maybe";
  holding_id: string | null;
  desire_id: string | null;
};
type Totals = {
  present: number;
  present_rollup: number;
  desired: number;
  missing: number;
  owned_elsewhere: number;
  to_buy: number;
};
type View = {
  collection: Summary;
  children: Summary[];
  cards: Row[];
  next_cursor: string | null;
  totals: Totals;
  commanders: {
    commanders: { oracle_id: string; name: string }[];
    color_identity: string[];
  } | null;
};
type TreeRow = { summary: Summary; present: number };

/// The machine route, which (unlike the server fn) takes a page size — the only
/// way to get a real mid-set cursor out of a small fixture.
async function viewOf(
  request: APIRequestContext,
  id: string,
  params: { q?: string; cursor?: string; limit?: number } = {},
): Promise<View> {
  const qs = new URLSearchParams();
  if (params.q !== undefined) qs.set("q", params.q);
  if (params.cursor !== undefined) qs.set("cursor", params.cursor);
  if (params.limit !== undefined) qs.set("limit", String(params.limit));
  const url = `/api/collections/${id}/view${qs.toString() ? `?${qs}` : ""}`;
  const res = await request.get(url);
  expect(res.status(), `GET ${url}`).toBe(200);
  return (await res.json()) as View;
}

async function tree(request: APIRequestContext): Promise<TreeRow[]> {
  const res = await request.get("/api/collections/tree");
  expect(res.status()).toBe(200);
  return ((await res.json()) as { collections: TreeRow[] }).collections;
}

/// A named seeded collection, by name — ids are per-database, so nothing here
/// hardcodes one.
async function collectionNamed(
  request: APIRequestContext,
  name: string,
): Promise<TreeRow> {
  const rows = await tree(request);
  const hit = rows.find((r) => r.summary.name === name);
  expect(
    hit,
    `dev seed should carry a collection named ${name} (scripts/seed-dev-data.sh)`,
  ).toBeTruthy();
  return hit!;
}

/// Copies in a collection and everything under it — the number a folder row and
/// the sidebar badge both show, computed here from the flat tree read so the
/// test derives it independently of the page's own walk.
function rolledUp(rows: TreeRow[], id: string): number {
  const own = rows.find((r) => r.summary.id === id)?.present ?? 0;
  return (
    own +
    rows
      .filter((r) => r.summary.parent_id === id)
      .reduce((sum, r) => sum + rolledUp(rows, r.summary.id), 0)
  );
}

const rowFor = (page: Page, oracleId: string) =>
  page.locator(`[data-testid="collection-row"][data-oracle="${oracleId}"]`);

/// Every rendered card row's three numeric columns, in document order.
///
/// HERE and WANTED are read from the count element rather than the cell: an
/// editable cell is a `CountStepper`, whose `textContent` also carries its
/// `−`/`+` button labels. `count-stepper-value` (editable) and
/// `here-count` / `wanted-placeholder` (not) are the two shapes per column, and
/// which one appears is itself part of the contract — so both columns report
/// their editability alongside their number.
async function renderedCells(page: Page) {
  return page.$$eval('[data-testid="collection-row"]', (trs) =>
    trs.map((tr) => {
      const cell = tr.querySelector('[data-testid="here-cell"]');
      const count = cell?.querySelector(
        '[data-testid="count-stepper-value"], [data-testid="here-count"]',
      );
      const rollup = cell?.querySelector('[data-testid="here-rollup"]');
      const wantCell = tr.querySelector('[data-testid="wanted-count"]');
      const want = wantCell?.querySelector(
        '[data-testid="count-stepper-value"], [data-testid="wanted-placeholder"]',
      );
      return {
        oracle: tr.getAttribute("data-oracle") ?? "",
        editable: !!cell?.querySelector('[data-testid="count-stepper"]'),
        here: (count?.textContent?.trim() ?? "") + (rollup?.textContent?.trim() ?? ""),
        wanted: want?.textContent?.trim() ?? "",
        wantedEditable: !!wantCell?.querySelector(
          '[data-testid="count-stepper"]',
        ),
        owned:
          tr.querySelector('[data-testid="owned-count"]')?.textContent?.trim() ??
          "",
      };
    }),
  );
}

/// What a row should render, derived from the API row — the expectation half.
/// Mirrors the spec's rules: WANTED is the still-needed count printed once per
/// card and board, OWNED collapses when equal to the here total, the rolled-up
/// part appended as `+n`.
function expectedCells(rows: Row[]) {
  const seen = new Set<string>();
  return rows.map((r) => {
    const key = `${r.oracle_id}/${r.board}`;
    const first = !seen.has(key);
    seen.add(key);
    const hereTotal = r.present + r.present_rollup;
    const here =
      (r.present > 0 ? String(r.present) : "—") +
      (r.present_rollup > 0 ? `+${r.present_rollup}` : "");
    // WANTED counts copies STILL NEEDED (maintainer ruling 2026-08-19), at
    // `(oracle, board)` grain on both sides — so the gap is measured against
    // what the whole card group holds, which the SERVER folds. Deriving it
    // here from the page's rows would reproduce the very page-boundary bug the
    // server fold exists to prevent, and the test would agree with the bug.
    const shortfall = Math.max(r.desired - r.present_group, 0);
    return {
      oracle: r.oracle_id,
      // Exactly the rows a single `holdings` row backs get the stepper: a cell
      // summing several finish/condition/language grains, or holding nothing,
      // is not addressable by one number.
      editable: r.holding_id !== null,
      here,
      // Printed once per card and board; zero is a number here, not a dash —
      // it is the create-a-want / keep-a-met-want affordance.
      wanted: first ? String(shortfall) : "—",
      // …and steppable wherever it prints, except the one refusal: a want
      // already spread over several `desires` rows has no single row a lone
      // number could mean. `desired === 0` is NOT a refusal — it is the
      // create-from-zero case.
      wantedEditable: first && !(r.desired > 0 && r.desire_id === null),
      owned: r.owned !== hereTotal ? String(r.owned) : "—",
    };
  });
}

/// The header's counts line, derived from the API's whole-collection totals.
function expectedCounts(t: Totals): string {
  const here = t.present + t.present_rollup;
  let out = `${here} here`;
  if (t.present_rollup > 0)
    out += ` (${t.present} own + ${t.present_rollup} rolled up)`;
  if (t.desired > 0) out += ` · ${t.desired} wanted`;
  return out;
}

// ------------------------------------------------------------------ reads ---

test("the binder view SSRs its rows and its header @fast", async ({
  request,
}) => {
  const trade = await collectionNamed(request, "Trade Binder");
  await expect(async () => {
    const raw = await (
      await request.get(`/my/collections/${trade.summary.id}`)
    ).text();
    expect(raw).toContain('data-testid="collection-table"');
    // Under the default out-of-order streaming this page would ship a skeleton
    // with the content parked in a <template>; assert the skeleton's absence.
    expect(raw).not.toContain('aria-label="Loading these cards"');

    const view = await viewOf(request, trade.summary.id);
    expect(view.cards.length).toBeGreaterThan(0);

    // EVERY row, not "at least one" — a `.take(1)` leaves the table, a row and
    // the first name all present (the trap `/my`'s mutation pass found).
    const ssrOracles = [
      ...raw.matchAll(/data-testid="collection-row" data-oracle="([0-9a-f-]+)"/g),
    ].map((m) => m[1]);
    expect(ssrOracles).toEqual(view.cards.map((r) => r.oracle_id));
    // The header counts the collection, server-side.
    expect(raw).toContain(expectedCounts(view.totals));
    expect(raw).toContain(view.collection.name);
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("the three columns agree with the collection read @fast", async ({
  page,
  request,
}) => {
  // Two collections, and the second one is the point. Against Trade Binder
  // alone this test was **vacuous for three of the rules it looks like it
  // covers**: no row anywhere in the fixture had `present_rollup > 0`, so the
  // `+n` marker and the OWNED-collapses-against-the-total rule were never
  // exercised, and no collection held two printings of one card, so
  // "WANTED prints once" was unfalsifiable. `Depth Box` (app/src/seed.rs
  // `build_depth`) exists for exactly those shapes; the guards below fail loudly
  // if a re-seed ever loses them, rather than letting the test go quiet again.
  const names = ["Trade Binder", "Depth Box"];
  await expect(async () => {
    let sawRollup = false;
    let sawRepeatedOracle = false;
    for (const name of names) {
      const c = await collectionNamed(request, name);
      const view = await viewOf(request, c.summary.id);
      sawRollup ||= view.cards.some((r) => r.present_rollup > 0);
      const keys = view.cards.map((r) => `${r.oracle_id}/${r.board}`);
      sawRepeatedOracle ||= new Set(keys).size < keys.length;

      await page.goto(`/my/collections/${c.summary.id}`);
      await hydrated(page);
      expect(await renderedCells(page), name).toEqual(expectedCells(view.cards));
    }
    expect(
      sawRollup,
      "dev seed must hold a card in both a collection and a descendant (build_depth)",
    ).toBe(true);
    expect(
      sawRepeatedOracle,
      "dev seed must hold two printings of one card in one collection (build_depth)",
    ).toBe(true);
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("a card the deck only wants is still a row @fast", async ({
  page,
  request,
}) => {
  // The FULL OUTER JOIN correction. Without it the deck's wants do not exist in
  // this view at all — so the failure asserted here is a *missing row*, not a
  // wrong number, and the needs chip would be counting cards the page cannot
  // show.
  const deck = await collectionNamed(request, "Commander Deck");
  await expect(async () => {
    const view = await viewOf(request, deck.summary.id);
    const wantOnly = view.cards.filter((r) => r.present === 0 && r.desired > 0);
    expect(
      wantOnly.length,
      "dev seed should have the deck wanting cards it does not hold",
    ).toBeGreaterThan(0);

    await page.goto(`/my/collections/${deck.summary.id}`);
    await hydrated(page);
    for (const row of wantOnly) {
      const tr = rowFor(page, row.oracle_id);
      await quick(tr).toHaveCount(1);
      await quick(tr.locator('[data-testid="here-cell"]')).toHaveText("—");
      // Read the WANTED number from its own count element, not the cell: where
      // the want is steppable the cell's text carries the `−`/`+` labels too.
      await quick(
        tr.locator(
          '[data-testid="wanted-count"] [data-testid="count-stepper-value"],' +
            ' [data-testid="wanted-count"] [data-testid="wanted-placeholder"]',
        ),
      ).toHaveText(String(row.desired));
      // No HERE stepper on a row with nothing here to step. (The WANTED cell
      // may well carry one — that is this column's whole point — so the scope
      // is the HERE cell, not the row.)
      await quick(
        tr.locator('[data-testid="here-cell"] [data-testid="count-stepper"]'),
      ).toHaveCount(0);
    }
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("child collections are folder rows above the cards, counted rolled-up @fast", async ({
  page,
  request,
}) => {
  // `Depth Box`, not `Shoebox`: Shoebox's only child is a leaf, so its
  // rolled-up count equals its own and this test could not tell the two apart
  // — a folder row rendering `present` instead of the rollup passed. `Depth
  // Shelf` has a child of its own (app/src/seed.rs `build_depth`), so the two
  // numbers differ and the guard below keeps it that way.
  const parent = await collectionNamed(request, "Depth Box");
  await expect(async () => {
    const rows = await tree(request);
    const kids = rows.filter((r) => r.summary.parent_id === parent.summary.id);
    expect(
      kids.length,
      "dev seed should nest a collection under Depth Box",
    ).toBeGreaterThan(0);
    expect(
      kids.some((k) => rolledUp(rows, k.summary.id) !== k.present),
      "dev seed's folder row must have descendants of its own (build_depth)",
    ).toBe(true);

    await page.goto(`/my/collections/${parent.summary.id}`);
    await hydrated(page);

    for (const kid of kids) {
      const folder = page.locator(
        `[data-testid="folder-row"][data-collection="${kid.summary.id}"]`,
      );
      await quick(folder).toHaveCount(1);
      await quick(folder.locator("a")).toHaveAttribute(
        "href",
        `/my/collections/${kid.summary.id}`,
      );
      // The rolled-up count, italic + dimmed — these copies are *there*.
      const cell = folder.locator('[data-testid="here-count"]');
      await quick(cell).toHaveText(String(rolledUp(rows, kid.summary.id)));
      await expect(cell).toHaveClass(/italic/);
    }

    // Above the cards: the first folder row precedes the first card row.
    const order = await page.$$eval(
      '[data-testid="folder-row"], [data-testid="collection-row"]',
      (els) => els.map((e) => e.getAttribute("data-testid")),
    );
    expect(order.indexOf("folder-row")).toBeLessThan(
      order.indexOf("collection-row"),
    );
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("the header counts the whole collection, and rolled-up copies are named @fast", async ({
  page,
  request,
}) => {
  // Shoebox holds one card itself and two under Rares, so its header is the
  // only place in the fixture where the `(own + rolled up)` parenthetical
  // appears — and the number must be the subtree's, not the page's.
  const shoebox = await collectionNamed(request, "Shoebox");
  await expect(async () => {
    const view = await viewOf(request, shoebox.summary.id);
    expect(
      view.totals.present_rollup,
      "dev seed should nest cards under Shoebox",
    ).toBeGreaterThan(0);
    // Not the page's sum: the rows on screen hold fewer copies than the header
    // claims, which is the whole distinction being asserted.
    const onPage = view.cards.reduce((s, r) => s + r.present, 0);
    expect(view.totals.present + view.totals.present_rollup).toBeGreaterThan(
      onPage,
    );

    await page.goto(`/my/collections/${shoebox.summary.id}`);
    await hydrated(page);
    await quick(page.locator('[data-testid="collection-counts"]')).toHaveText(
      expectedCounts(view.totals),
    );
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("the needs chip says what the collection's needs say @fast", async ({
  page,
  request,
}) => {
  const deck = await collectionNamed(request, "Commander Deck");
  await expect(async () => {
    const view = await viewOf(request, deck.summary.id);
    // Cross-checked against the needs read — a different query over the same
    // rows — rather than against the projection the chip is rendered from.
    const needs = (await (
      await request.get(`/api/collections/${deck.summary.id}/needs`)
    ).json()) as {
      rows: { desired: number; present_here: number; owned_elsewhere: number; short: number }[];
    };
    const missing = needs.rows.reduce(
      (s, r) => s + (r.desired - r.present_here),
      0,
    );
    const elsewhere = needs.rows.reduce((s, r) => s + r.owned_elsewhere, 0);
    const toBuy = needs.rows.reduce((s, r) => s + r.short, 0);
    expect(missing, "dev seed should leave the deck incomplete").toBeGreaterThan(
      0,
    );
    expect(view.totals.missing).toBe(missing);
    expect(view.totals.owned_elsewhere).toBe(elsewhere);
    expect(view.totals.to_buy).toBe(toBuy);

    await page.goto(`/my/collections/${deck.summary.id}`);
    await hydrated(page);
    const chip = page.locator('[data-testid="needs-chip"]');
    await quick(chip).toContainText(
      `${missing} missing — ${elsewhere} owned elsewhere · ${toBuy} to buy`,
    );
    await quick(chip).toHaveAttribute(
      "href",
      `/my/collections/${deck.summary.id}/needs`,
    );
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("the deck-only header parts are absent from a binder @fast", async ({
  page,
  request,
}) => {
  // The deck-only half of the variant, asserted by its absence — otherwise
  // "the deck shows X" passes for a page that shows X everywhere.
  //
  // The needs chip is deliberately NOT in this list. It is deck-only in
  // specs/app-ui.md's distilled bullet, but the design authority that bullet
  // distills says otherwise — design/information-architecture.md: "Needs views
  // and pick lists are contextual only: reached from the needs chip on a deck
  // **or collection** header" — and `/my/collections/:id/needs` is a route for
  // any collection, so gating the chip on decks would strand it. The
  // binder-with-wants case below pins that reading.
  const trade = await collectionNamed(request, "Trade Binder");
  await page.goto(`/my/collections/${trade.summary.id}`);
  await hydrated(page);
  await expect(page.locator('[data-testid="collection-kind"]')).toHaveText(
    "Binder",
  );
  await expect(page.locator('[data-testid="collection-format"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="deck-commanders"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="teardown-open"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="deck-section"]')).toHaveCount(0);
});

test("a binder that wants cards gets the needs chip too @fast", async ({
  page,
  request,
}) => {
  // The other side of the rule above, and the reason it needs its own test: the
  // absence assertion against Trade Binder passes for free (it has no desires
  // at all), so no chip-related mutation could ever kill it. `Depth Box` wants
  // more copies than it holds, so its chip is a real render.
  const box = await collectionNamed(request, "Depth Box");
  await expect(async () => {
    const view = await viewOf(request, box.summary.id);
    expect(
      view.totals.missing,
      "dev seed's Depth Box should want more than it holds (build_depth)",
    ).toBeGreaterThan(0);
    expect(view.collection.kind).toBe("binder");

    await page.goto(`/my/collections/${box.summary.id}`);
    await hydrated(page);
    const chip = page.locator('[data-testid="needs-chip"]');
    await quick(chip).toContainText(`${view.totals.missing} missing`);
    await quick(chip).toHaveAttribute(
      "href",
      `/my/collections/${box.summary.id}/needs`,
    );
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("the deck header carries its format and commander @fast", async ({
  page,
  request,
}) => {
  const deck = await collectionNamed(request, "Commander Deck");
  await expect(async () => {
    const view = await viewOf(request, deck.summary.id);
    expect(view.collection.format).toBeTruthy();
    const cmd = (await (
      await request.get(`/api/collections/${deck.summary.id}/commanders`)
    ).json()) as {
      commanders: { name: string }[];
      color_identity: string[];
    };
    expect(
      cmd.commanders.length,
      "dev seed should tag a commander",
    ).toBeGreaterThan(0);

    // The independent witness. `/commanders` stopped being one in this change:
    // it was refactored to call the same `commanders_in` helper `collection_view`
    // uses, so a mutation there moves both sides together. `cards_with_tag` is a
    // genuinely different query over `card_tags`, reached by resolving the
    // built-in tag by slug rather than by trusting either commander read.
    const tags = (await (
      await request.get(`/api/collections/${deck.summary.id}/tags`)
    ).json()) as { id: string; builtin: string | null }[];
    const commanderTag = tags.find((t) => t.builtin === "commander");
    expect(commanderTag, "the commander built-in tag should be in scope").toBeTruthy();
    const tagged = (await (
      await request.get(
        `/api/collections/${deck.summary.id}/tags/${commanderTag!.id}/cards`,
      )
    ).json()) as { name: string }[];
    expect(tagged.map((c) => c.name).sort()).toEqual(
      cmd.commanders.map((c) => c.name).sort(),
    );
    expect(view.commanders!.commanders.map((c) => c.name).sort()).toEqual(
      tagged.map((c) => c.name).sort(),
    );

    await page.goto(`/my/collections/${deck.summary.id}`);
    await hydrated(page);
    await quick(page.locator('[data-testid="collection-kind"]')).toHaveText(
      "Deck",
    );
    await quick(page.locator('[data-testid="collection-format"]')).toHaveText(
      view.collection.format!,
    );
    const card = page.locator('[data-testid="deck-commanders"]');
    await quick(card).toHaveCount(1);
    for (const c of cmd.commanders) {
      await quick(card.locator('[data-testid="deck-commander"]', { hasText: c.name })).toHaveCount(1);
    }
    await quick(page.locator('[data-testid="deck-color-identity"]')).toHaveText(
      cmd.color_identity.join(""),
    );
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("deck cards are grouped by type, sideboard last, with slot counts @fast", async ({
  page,
  request,
}) => {
  const deck = await collectionNamed(request, "Commander Deck");
  await expect(async () => {
    const view = await viewOf(request, deck.summary.id);

    // Independent expectation: bucket by the same decklist convention, count
    // copies present plus the copies wanted and missing (each card once).
    const bucket = (t: string | null) => {
      const line = t ?? "";
      for (const name of [
        "Creature",
        "Planeswalker",
        "Instant",
        "Sorcery",
        "Artifact",
        "Enchantment",
        "Battle",
        "Land",
      ]) {
        if (line.includes(name)) return name;
      }
      return "Other";
    };
    const plural: Record<string, string> = {
      Creature: "Creatures",
      Planeswalker: "Planeswalkers",
      Instant: "Instants",
      Sorcery: "Sorceries",
      Artifact: "Artifacts",
      Enchantment: "Enchantments",
      Battle: "Battles",
      Land: "Lands",
      Other: "Other",
    };
    const boardLabel: Record<string, string> = {
      main: "",
      side: "Sideboard · ",
      maybe: "Maybeboard · ",
    };
    const groups = new Map<string, Row[]>();
    for (const r of view.cards) {
      const key = `${boardLabel[r.board]}${plural[bucket(r.type_line)]}`;
      groups.set(key, [...(groups.get(key) ?? []), r]);
    }
    const slotsOf = (rows: Row[]) => {
      let slots = rows.reduce((s, r) => s + r.present, 0);
      const counted = new Set<string>();
      for (const r of rows) {
        const key = `${r.oracle_id}/${r.board}`;
        if (r.desired <= 0 || counted.has(key)) continue;
        counted.add(key);
        const held = rows
          .filter((o) => o.oracle_id === r.oracle_id && o.board === r.board)
          .reduce((s, o) => s + o.present, 0);
        slots += Math.max(r.desired - held, 0);
      }
      return slots;
    };
    expect(
      [...groups.keys()].some((k) => k.startsWith("Sideboard")),
      "dev seed should give the deck a sideboard card",
    ).toBe(true);

    await page.goto(`/my/collections/${deck.summary.id}`);
    await hydrated(page);

    const rendered = await page.$$eval("[data-section]", (els) =>
      els.map((e) => ({
        label: e.getAttribute("data-section") ?? "",
        text: (e.textContent ?? "").replace(/\s+/g, " ").trim(),
      })),
    );
    expect(rendered.map((r) => r.label).sort()).toEqual(
      [...groups.keys()].sort(),
    );
    for (const r of rendered) {
      expect(r.text, `slot count for ${r.label}`).toBe(
        `${r.label} · ${slotsOf(groups.get(r.label)!)}`,
      );
    }
    // Boards read in order: every mainboard section precedes every other.
    const firstOther = rendered.findIndex((r) => r.label.includes(" · "));
    if (firstOther >= 0) {
      expect(
        rendered.slice(firstOther).every((r) => r.label.includes(" · ")),
        "mainboard sections must all precede the sideboard",
      ).toBe(true);
    }
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("the add default is Want in a deck and Have in a binder @fast", async ({
  page,
  request,
}) => {
  // specs/app-ui.md: the deck variant is Want-led; "binders and Inbox are
  // Have-led" — one condition, since the Inbox is a binder.
  const deck = await collectionNamed(request, "Commander Deck");
  const trade = await collectionNamed(request, "Trade Binder");
  const inbox = (await tree(request)).find((r) => r.summary.is_inbox)!;

  await page.goto(`/my/collections/${deck.summary.id}`);
  await hydrated(page);
  await expect(page.locator('[data-testid="add-default"]')).toContainText(
    "Want",
  );
  for (const binder of [trade.summary.id, inbox.summary.id]) {
    await page.goto(`/my/collections/${binder}`);
    await hydrated(page);
    await expect(page.locator('[data-testid="add-default"]')).toContainText(
      "Have",
    );
  }
});

test("`/` focuses the in-collection search, but not while typing @fast", async ({
  page,
  request,
}) => {
  const trade = await collectionNamed(request, "Trade Binder");
  await page.goto(`/my/collections/${trade.summary.id}`);
  await hydrated(page);
  const box = page.locator("#collection-query");

  await page.locator('[data-testid="collection-title"]').click();
  await page.keyboard.press("/");
  await expect(box).toBeFocused();
  // And the key that focused it is not also typed into it.
  await expect(box).toHaveValue("");

  // With the caret already in a field, `/` is just a character — otherwise a
  // card name containing a slash (`Fire // Ice`) would be untypable.
  await box.fill("fire ");
  await page.keyboard.press("/");
  await expect(box).toHaveValue("fire /");
});

test("the back link targets the parent collection @fast", async ({
  page,
  request,
}) => {
  // Drill-down, not history: `back` targets the *parent collection*, which is
  // what "back walks up the tree" means (design/information-architecture.md).
  // This runs at desktop width and so only pins the `href`; the mobile block
  // below is what proves the affordance is the one you actually get on a phone.
  const rares = await collectionNamed(request, "Rares");
  const shoebox = await collectionNamed(request, "Shoebox");
  expect(rares.summary.parent_id).toBe(shoebox.summary.id);

  await page.goto(`/my/collections/${rares.summary.id}`);
  await hydrated(page);
  const back = page.locator('[data-testid="collection-back"]');
  await expect(back).toHaveAttribute(
    "href",
    `/my/collections/${shoebox.summary.id}`,
  );
  await expect(back).toContainText(shoebox.summary.name);

  // A top-level collection goes up to the root screen instead — `/my`, which
  // below `md` is the My-cards drill-down list (app/src/my/root.rs).
  await page.goto(`/my/collections/${shoebox.summary.id}`);
  await hydrated(page);
  await expect(page.locator('[data-testid="collection-back"]')).toHaveAttribute(
    "href",
    "/my",
  );
});

test.describe("mobile", () => {
  // The acceptance criterion is "mobile drill-down", and asserting an `href` at
  // desktop width does not test it: `collection-back` is in the DOM at every
  // width (it is `md:hidden`, a CSS concern), and the breadcrumb it replaces is
  // in the DOM too. Only a narrow viewport can tell which of the two a phone
  // user is actually offered.
  test.use({ viewport: { width: 390, height: 844 } });

  test("drill-down replaces the breadcrumb and walks back up @fast", async ({
    page,
    request,
  }) => {
    const box = await collectionNamed(request, "Depth Box");
    const shelf = await collectionNamed(request, "Depth Shelf");

    await page.goto(`/my/collections/${box.summary.id}`);
    await hydrated(page);
    await expect(page.locator('nav[aria-label="Breadcrumb"]')).toBeHidden();
    await expect(page.locator('[data-testid="collection-back"]')).toBeVisible();

    // Tapping a folder row drills in…
    await page
      .locator(`[data-testid="folder-row"][data-collection="${shelf.summary.id}"] a`)
      .click();
    await page.waitForURL(`/my/collections/${shelf.summary.id}`);
    await hydrated(page);
    await expect(page.locator('[data-testid="collection-title"]')).toHaveText(
      shelf.summary.name,
    );

    // …and back walks up to the parent, not to wherever history was.
    await page.locator('[data-testid="collection-back"]').click();
    await page.waitForURL(`/my/collections/${box.summary.id}`);
    await hydrated(page);
    await expect(page.locator('[data-testid="collection-title"]')).toHaveText(
      box.summary.name,
    );
  });

  test("no collection table scrolls sideways at phone width @fast", async ({
    page,
    request,
  }) => {
    // Six columns in a table on a 390px screen is exactly the arrangement that
    // overflows; the Type and Mana columns are `hidden md:table-cell` /
    // `hidden sm:table-cell` to stop it.
    //
    // **Measure the scroll container, not the document.** `TableWrapper` is
    // `overflow-auto` (app/src/components/ui/table.rs), so a too-wide table
    // becomes a wrapper-local scrollbar that never moves
    // `document.documentElement` at all: the document-level version of this
    // assertion read 0 while the table itself scrolled 92–128px, and passed
    // with the progressive columns deleted — vacuous for the exact failure it
    // names. The document check stays as a second, cheaper net (page chrome
    // outside the table), but the wrapper is the one that can fail.
    for (const name of ["Depth Box", "Commander Deck", "Bulk Box"]) {
      const c = await collectionNamed(request, name);
      await page.goto(`/my/collections/${c.summary.id}`);
      await hydrated(page);

      const table = page.locator('[data-testid="collection-table"]');
      await expect(table).toHaveCount(1);
      const wrapper = await table.evaluate((el) => {
        const w = el.closest('[data-name="TableWrapper"]');
        if (!w) throw new Error("the table has no TableWrapper to scroll in");
        return { overflow: w.scrollWidth - w.clientWidth, client: w.clientWidth };
      });
      expect(wrapper.client, `${name} wrapper has no width`).toBeGreaterThan(0);
      expect(
        wrapper.overflow,
        `${name}'s table scrolls sideways inside its wrapper`,
      ).toBeLessThanOrEqual(1);

      const doc = await page.evaluate(
        () =>
          document.documentElement.scrollWidth -
          document.documentElement.clientWidth,
      );
      expect(doc, `${name} overflows the document`).toBeLessThanOrEqual(1);
    }
  });
});

test("quick search filters this collection and rides the URL @fast", async ({
  page,
  request,
}) => {
  const bulk = await collectionNamed(request, "Bulk Box");
  const id = bulk.summary.id;
  await expect(async () => {
    const all = await viewOf(request, id);
    expect(all.cards.length).toBeGreaterThan(1);
    const needle = all.cards[0].name.slice(0, 6);
    const expected = await viewOf(request, id, { q: needle });
    expect(expected.cards.length).toBeGreaterThan(0);
    expect(expected.cards.length).toBeLessThan(all.cards.length);

    await page.goto(`/my/collections/${id}`);
    await hydrated(page);
    await page.locator("#collection-query").fill(needle);

    // The URL is the query, and the rows follow the URL.
    await quick(page).toHaveURL(
      `/my/collections/${id}?q=${encodeURIComponent(needle)}`,
    );
    await quick(page.locator('[data-testid="collection-row"]')).toHaveCount(
      expected.cards.length,
    );
    await quick(rowFor(page, expected.cards[0].oracle_id)).toBeVisible();
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("a `?q=` refresh keeps the page mounted, focused and filtered @fast", async ({
  page,
  request,
}) => {
  // P6-068. `CollectionPage` used to read its own `Resource` in its *setup
  // body*, which registers on the nearest `SuspenseContext` — `RequireAuth`'s
  // `<Suspense>`, an ancestor of the page — so every debounced keystroke
  // re-suspended the auth guard and `EitherKeepAlive` unmounted this whole
  // subtree for the length of the fetch, blurring the field and silently hiding
  // any showing native popover.
  //
  // **"Is it still the same node" cannot see that defect**: keep-alive
  // re-inserts the *identical* nodes rather than rebuilding them, so a stamp
  // survives either way. The observable is that the element leaves the document
  // at all — watched with a `MutationObserver` on `removedNodes` rather than by
  // polling `isConnected`, so a detach that is over before the next poll still
  // counts. The stamp is asserted too, but only for what it does prove: that
  // nothing rebuilt the subtree either.
  const bulk = await collectionNamed(request, "Bulk Box");
  const id = bulk.summary.id;
  await expect(async () => {
    const all = await viewOf(request, id);
    expect(all.cards.length).toBeGreaterThan(1);
    const needle = all.cards[0].name.slice(0, 6);
    const expected = await viewOf(request, id, { q: needle });
    expect(expected.cards.length).toBeGreaterThan(0);
    expect(expected.cards.length).toBeLessThan(all.cards.length);

    await page.goto(`/my/collections/${id}`);
    await hydrated(page);
    const box = page.locator("#collection-query");
    await box.click();
    await expect(box).toBeFocused();

    await page.evaluate(() => {
      const el = document.querySelector('[data-testid="collection-page"]')!;
      (el as unknown as { __p6068?: string }).__p6068 = "stamped";
      const state = { detaches: 0 };
      (window as unknown as { __p6068: typeof state }).__p6068 = state;
      new MutationObserver((records) => {
        for (const record of records) {
          for (const gone of Array.from(record.removedNodes)) {
            if (gone === el || (gone as Element).contains?.(el)) {
              state.detaches += 1;
            }
          }
        }
      }).observe(document.body, { childList: true, subtree: true });
    });

    // Several characters at 120 ms — the 250 ms trailing debounce collapses
    // the burst into a single real `?q=` navigation, which is all the
    // assertion needs (one detach ≠ 0).
    await box.pressSequentially(needle, { delay: 120 });

    // The rows followed the URL, i.e. the navigations and their fetches really
    // happened — without this the detach count below would be vacuously zero.
    await quick(page).toHaveURL(
      `/my/collections/${id}?q=${encodeURIComponent(needle)}`,
    );
    await quick(page.locator('[data-testid="collection-row"]')).toHaveCount(
      expected.cards.length,
    );

    const observed = await page.evaluate(() => ({
      detaches: (window as unknown as { __p6068: { detaches: number } })
        .__p6068.detaches,
      stamp: (
        document.querySelector('[data-testid="collection-page"]') as unknown as {
          __p6068?: string;
        } | null
      )?.__p6068,
    }));
    expect(
      observed.detaches,
      "the page subtree was removed from the document during a `?q=` refresh",
    ).toBe(0);
    expect(observed.stamp, "the page subtree was rebuilt").toBe("stamped");
    // The caret is still in the field — no interval puts it back now.
    await expect(box).toBeFocused();
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

test("a wildcard typed into the in-collection search is literal @fast", async ({
  page,
  request,
}) => {
  // `%` is a LIKE wildcard; unescaped it would match every card in here
  // instead of none — the same escaping helper `/catalog` and `/my` use.
  const trade = await collectionNamed(request, "Trade Binder");
  await page.goto(`/my/collections/${trade.summary.id}?q=%25`);
  await hydrated(page);
  await expect(page.locator('[data-testid="collection-empty"]')).toContainText(
    "Nothing in here matches that search",
  );
  await expect(page.locator('[data-testid="collection-table"]')).toHaveCount(0);
});

test("a junk cursor is a rendered error, not a crash @fast", async ({
  page,
  request,
}) => {
  const trade = await collectionNamed(request, "Trade Binder");
  await page.goto(`/my/collections/${trade.summary.id}?cursor=not-a-cursor`);
  await hydrated(page);
  await expect(page.locator('[data-testid="collection-error"]')).toContainText(
    "Couldn't load this collection",
  );
});

test("?cursor= is honored on a cold load @fast", async ({ page, request }) => {
  const bulk = await collectionNamed(request, "Bulk Box");
  const id = bulk.summary.id;
  await expect(async () => {
    const first = await viewOf(request, id, { limit: 3 });
    expect(first.next_cursor, "a 3-row page must not be the last").toBeTruthy();
    const rest = await viewOf(request, id, { cursor: first.next_cursor! });
    expect(rest.cards.length).toBeGreaterThan(0);

    // Request level: the cursor page must be in the raw response too.
    const url = `/my/collections/${id}?cursor=${encodeURIComponent(first.next_cursor!)}`;
    const raw = await (await request.get(url)).text();
    expect(raw).toContain(`data-oracle="${rest.cards[0].oracle_id}"`);
    expect(raw).not.toContain(`data-oracle="${first.cards[0].oracle_id}"`);

    await page.goto(url);
    await hydrated(page);
    for (const row of first.cards) {
      await quick(rowFor(page, row.oracle_id)).toHaveCount(0);
    }
    await quick(page.locator('[data-testid="page-first"]')).toHaveCount(1);
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
});

// ----------------------------------------------------------------- writes ---

let scratchSeq = 0;
/// A unique scratch name — worker index plus a per-file counter, no wall clock,
/// so parallel workers and the three browser projects cannot collide.
function scratchName(prefix: string): string {
  return `zz-e2e-${prefix}-w${test.info().workerIndex}-${++scratchSeq}`;
}

async function createCollection(
  request: APIRequestContext,
  kind: "binder" | "deck",
  name: string,
  format: string | null = null,
): Promise<string> {
  const res = await request.post("/api/collections", {
    data: { parent_id: null, kind, name, format },
  });
  expect(res.status(), `create ${name}`).toBe(200);
  return ((await res.json()) as Summary).id;
}

async function deleteCollection(request: APIRequestContext, id: string) {
  await request.post(`/api/collections/${id}/delete`, { data: {} });
}

/// Put one copy of a printing into a collection, at the default grain — an
/// intake, so it comes from nowhere and vanishes again when the scratch
/// collection is deleted (delete cascades holdings).
async function addHave(
  request: APIRequestContext,
  id: string,
  printingId: string,
  quantity = 1,
) {
  const res = await request.post(`/api/collections/${id}/have`, {
    data: { printing_id: printingId, quantity },
  });
  expect(res.status(), "add have").toBe(200);
}

/// Want a card in a collection — the desire-only row shape the WANTED column
/// exists for. Oracle-grained, with no printing pin, so exactly one `desires`
/// row backs the cell and the stepper has something to address.
async function addWant(
  request: APIRequestContext,
  id: string,
  oracleId: string,
  quantity = 1,
) {
  const res = await request.post(`/api/collections/${id}/want`, {
    data: { oracle_id: oracleId, quantity },
  });
  expect(res.status(), "add want").toBe(200);
}

/// A row's WANTED number, wherever it lives. The cell is a `CountStepper` where
/// the want is editable, and its `textContent` would carry the `−`/`+` labels
/// too — so read the count element, whichever of the two shapes rendered.
const wantedValue = (tr: ReturnType<typeof rowFor>) =>
  tr.locator(
    '[data-testid="wanted-count"] [data-testid="count-stepper-value"],' +
      ' [data-testid="wanted-count"] [data-testid="wanted-placeholder"]',
  );

/// Some printing the fixture already holds — any will do; the tests below only
/// move it inside their own scratch collections.
async function somePrinting(request: APIRequestContext): Promise<Row> {
  const trade = await collectionNamed(request, "Trade Binder");
  const view = await viewOf(request, trade.summary.id);
  expect(view.cards.length).toBeGreaterThan(0);
  return view.cards[0];
}

test("the stepper edits HERE in place, and the header follows it @fast", async ({
  page,
  request,
}) => {
  const card = await somePrinting(request);
  const scratch = await createCollection(
    request,
    "binder",
    scratchName("stepper"),
  );
  try {
    await addHave(request, scratch, card.printing_id, 2);
    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);

    const tr = rowFor(page, card.oracle_id);
    await expect(tr).toHaveCount(1);
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "2 here",
    );

    // Step up, then blur out of the stepper: one editing session, one commit.
    await tr.locator(HERE_INC).click();
    await page.locator('[data-testid="collection-title"]').click();

    // Both numbers move, and the *database* moved — asserting only the DOM
    // would pass with the save stubbed out.
    await expect(tr.locator(HERE_VALUE)).toHaveText(
      "3",
    );
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "3 here",
    );
    await expect(async () => {
      const after = await viewOf(request, scratch);
      expect(after.cards[0].present).toBe(3);
      expect(after.totals.present).toBe(3);
    }).toPass({ timeout: 10_000 });

    // The sidebar badge follows too — a commit refetches the collection tree
    // that feeds it. Waiting for it here is not incidental: it is the point at
    // which the refetch has definitely landed, and the stepper below it must
    // STILL read 3. When the whole table awaited that resource, the refetch
    // remounted every row and re-seeded the stepper from the stale fetched
    // count, which silently disarmed the undo toast (its `from` already
    // matched). This pair is that regression's tripwire.
    await expect(
      page.locator(`[data-tree-row="${scratch}"] [data-name="Badge"]`).first(),
    ).toHaveText("3");
    await expect(tr.locator(HERE_VALUE)).toHaveText(
      "3",
    );

    // The undo toast reverses it through the same channel.
    const toast = page.locator('[data-name="Toast"]', { hasText: "2 → 3" });
    await expect(toast).toBeVisible();
    await toast.getByRole("button", { name: "Undo" }).click();
    await expect(tr.locator(HERE_VALUE)).toHaveText(
      "2",
    );
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "2 here",
    );
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].present).toBe(2);
    }).toPass({ timeout: 10_000 });

    // A reload agrees: the optimistic number was not the only thing that moved.
    await page.reload();
    await hydrated(page);
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "2 here",
    );
  } finally {
    await deleteCollection(request, scratch);
  }
});

// The alpha report this pair exists for: "there's a stepper for changing the
// quantity of cards 'here' in a collection, but not for 'Wanted'". WANTED is
// the same `CountStepper`, wired to `set_desire_quantity` through the shared
// `WantStepper` that `/cards/:id`'s "Your wants" rows already used — so these
// two tests are the collection-table half of `card-detail.spec.ts`'s want
// stepper pair, and they assert the two things a shared component cannot bring
// with it: this page's own header clause, and that the write reaches the row
// the *table* addressed.

test("the WANTED stepper edits a want in place from the table, and a reload agrees @fast", async ({
  page,
  request,
}) => {
  const card = await somePrinting(request);
  const scratch = await createCollection(
    request,
    "binder",
    scratchName("want-stepper"),
  );
  try {
    await addWant(request, scratch, card.oracle_id, 2);
    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);

    const tr = rowFor(page, card.oracle_id);
    await expect(tr).toHaveCount(1);
    await expect(wantedValue(tr)).toHaveText("2");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "0 here · 2 wanted",
    );
    // The row holds nothing, so HERE is the non-editable shape — which is also
    // what makes the stepper found below unambiguously the WANTED one.
    await expect(
      tr.locator('[data-testid="here-cell"] [data-testid="count-stepper"]'),
    ).toHaveCount(0);

    // Step up, then blur out of the stepper: one editing session, one commit.
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-inc"]')
      .click();
    await page.locator('[data-testid="collection-title"]').click();

    // The cell, the header clause, and the *database* — asserting only the DOM
    // would pass with the save stubbed out.
    await expect(wantedValue(tr)).toHaveText("3");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "0 here · 3 wanted",
    );
    await expect(async () => {
      const after = await viewOf(request, scratch);
      expect(after.cards[0].desired).toBe(3);
      expect(after.totals.desired).toBe(3);
    }).toPass({ timeout: 10_000 });

    // A reload agrees: the optimistic number was not the only thing that moved.
    await page.reload();
    await hydrated(page);
    await expect(wantedValue(rowFor(page, card.oracle_id))).toHaveText("3");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "0 here · 3 wanted",
    );
  } finally {
    await deleteCollection(request, scratch);
  }
});

test("committing a WANTED cell to zero drops the want, with no Undo offered @fast", async ({
  page,
  request,
}) => {
  // Desires carry no ledger (`shared::QuickAddReceipt`'s own doc: a `+ Want`
  // is confirmed but never undoable), so a committed zero here is a direct
  // delete — unlike the HERE stepper's reversible move, which offers Undo.
  const card = await somePrinting(request);
  const scratch = await createCollection(
    request,
    "binder",
    scratchName("want-zero"),
  );
  try {
    await addWant(request, scratch, card.oracle_id, 1);
    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);

    const tr = rowFor(page, card.oracle_id);
    await expect(wantedValue(tr)).toHaveText("1");
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-dec"]')
      .click();
    await page.locator('[data-testid="collection-title"]').click();

    // The cell stays a stepper reading 0 — that zero is the affordance that
    // lets you want the card again — and the header's wanted clause goes,
    // rather than standing stale at "1 wanted" until a reload.
    await expect(wantedValue(tr)).toHaveText("0");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "0 here",
    );

    const toast = page.locator('[data-name="Toast"]', {
      hasText: `Removed ${card.name} from wants`,
    });
    await expect(toast).toBeVisible();
    await expect(toast.getByRole("button", { name: "Undo" })).toHaveCount(0);

    await expect(async () => {
      const after = await viewOf(request, scratch);
      expect(after.cards).toEqual([]);
      expect(after.totals.desired).toBe(0);
    }).toPass({ timeout: 10_000 });
  } finally {
    await deleteCollection(request, scratch);
  }
});

test("a met want is a steppable 0, and stepping it back to 0 keeps the want @fast", async ({
  page,
  request,
}) => {
  // Maintainer ruling 2026-08-19, rule 3: WANTED counts copies still needed,
  // so a want you have already filled reads 0 — and stepping that 0 sets the
  // target to what is held rather than deleting the row. A want means "keep N
  // of these here"; forgetting one outright stays card detail's job.
  const card = await somePrinting(request);
  const scratch = await createCollection(
    request,
    "binder",
    scratchName("met-want"),
  );
  try {
    await addHave(request, scratch, card.printing_id, 2);
    await addWant(request, scratch, card.oracle_id, 2);
    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);

    const tr = rowFor(page, card.oracle_id);
    // A steppable zero, not the `—` the pre-ruling rule collapsed this to.
    await expect(wantedValue(tr)).toHaveText("0");
    await expect(
      tr.locator('[data-testid="wanted-count"] [data-testid="count-stepper"]'),
    ).toHaveCount(1);

    // Ask for one more than is here: the target becomes held + 1 = 3.
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-inc"]')
      .click();
    await page.locator('[data-testid="collection-title"]').click();
    await expect(wantedValue(tr)).toHaveText("1");
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].desired).toBe(3);
    }).toPass({ timeout: 10_000 });

    // …and back down to nothing-still-needed. The want survives at the level
    // held (2); it is NOT deleted, which is the half of the ruling this test
    // exists for.
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-dec"]')
      .click();
    await page.locator('[data-testid="collection-title"]').click();
    await expect(wantedValue(tr)).toHaveText("0");
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].desired).toBe(2);
    }).toPass({ timeout: 10_000 });

    // Card detail still lists the want — the surface that speaks in
    // quantities rather than gaps, and the one that can delete it.
    const detail = await request.get(`/api/cards/${card.oracle_id}`);
    expect(detail.status()).toBe(200);
    const wants = ((await detail.json()) as {
      wants: { collection_id: string; quantity: number }[] | null;
    }).wants;
    expect(wants?.find((w) => w.collection_id === scratch)?.quantity).toBe(2);

    await page.reload();
    await hydrated(page);
    await expect(wantedValue(rowFor(page, card.oracle_id))).toHaveText("0");
  } finally {
    await deleteCollection(request, scratch);
  }
});

test("stepping WANTED up on a card nothing is wanted for creates the want @fast", async ({
  page,
  request,
}) => {
  // The create-from-zero case (ruling rule 3): no `desires` row exists, so the
  // commit goes through `create_desire` rather than `set_desire_quantity`, and
  // the stepper rewires to the row it made — a second step in the same session
  // must SET that row, not create-and-increment a second time.
  const card = await somePrinting(request);
  const scratch = await createCollection(
    request,
    "binder",
    scratchName("want-create"),
  );
  try {
    await addHave(request, scratch, card.printing_id, 1);
    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);

    const tr = rowFor(page, card.oracle_id);
    await expect(wantedValue(tr)).toHaveText("0");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "1 here",
    );

    // Ask for two more than the one already here → target 3.
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-inc"]')
      .click();
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-inc"]')
      .click();
    await page.locator('[data-testid="collection-title"]').click();
    await expect(wantedValue(tr)).toHaveText("2");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "1 here · 3 wanted",
    );
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].desired).toBe(3);
    }).toPass({ timeout: 10_000 });

    // A second edit in the same session must land on the row the first one
    // created — 4, not 3 + 4.
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-inc"]')
      .click();
    await page.locator('[data-testid="collection-title"]').click();
    await expect(wantedValue(tr)).toHaveText("3");
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].desired).toBe(4);
    }).toPass({ timeout: 10_000 });

    await page.reload();
    await hydrated(page);
    await expect(wantedValue(rowFor(page, card.oracle_id))).toHaveText("3");
  } finally {
    await deleteCollection(request, scratch);
  }
});

test("the want-set endpoint sets rather than adds, so a stale stepper cannot compound @fast", async ({
  request,
}) => {
  // Review round 2, MAJOR. The WANTED stepper commits an ABSOLUTE target it
  // computed from what it believes is there, so the endpoint behind
  // create-from-zero must SET. With `+ Want`'s incrementing upsert, a want
  // created in another tab since the page loaded — or a second commit racing
  // the first, before the created row's id comes back — lands a quantity
  // nobody asked for.
  //
  // At the API level, because that is where the two semantics differ: the
  // stepper cannot be made to race itself reliably from a browser.
  const card = await somePrinting(request);
  const scratch = await createCollection(
    request,
    "binder",
    scratchName("want-set"),
  );
  try {
    // Someone else's write lands first…
    await addWant(request, scratch, card.oracle_id, 3);
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].desired).toBe(3);
    }).toPass({ timeout: 10_000 });

    // …and the stepper's own create, which believes it is starting from
    // nothing, asks for 2. SET semantics: the answer is 2, not 5.
    const set = await request.post(`/api/collections/${scratch}/want/set`, {
      data: { oracle_id: card.oracle_id, quantity: 2 },
    });
    expect(set.status(), "want/set").toBe(200);
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].desired).toBe(2);
    }).toPass({ timeout: 10_000 });

    // Twice more with the same body: setting is idempotent, adding would not be.
    for (const q of [2, 2]) {
      const again = await request.post(`/api/collections/${scratch}/want/set`, {
        data: { oracle_id: card.oracle_id, quantity: q },
      });
      expect(again.status()).toBe(200);
    }
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].desired).toBe(2);
    }).toPass({ timeout: 10_000 });

    // …while `+ Want` keeps its own gesture semantics, untouched: pressing it
    // again means "and one more". This is the positive control — without it
    // the assertions above would also pass if both routes had become SET.
    await addWant(request, scratch, card.oracle_id, 1);
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards[0].desired).toBe(3);
    }).toPass({ timeout: 10_000 });
  } finally {
    await deleteCollection(request, scratch);
  }
});

test("the WANTED gap is measured against the card's whole held count here @fast", async ({
  page,
  request,
}) => {
  // Review round 2, BLOCKER. `desired − held` is `(oracle, board)`-grained on
  // both sides, and `held` must be the card's total in this collection — the
  // server's `present_group`, not a fold over the rendered rows, which are one
  // keyset page and can split a multi-printing card across two of them. The
  // gap is not only printed from that number, it is WRITTEN back through it
  // (`desired' = held + gap`), so a fold would save a wrong quantity.
  //
  // Two printings of ONE card in one collection is the shape that tells a
  // group total from a row's own `present` at all — `Depth Box` is the seeded
  // collection that has it (`app/src/seed.rs` `build_depth`), and the guard
  // below fails loudly rather than skipping if a re-seed ever loses it. (The
  // page-boundary case itself is pinned as arithmetic in
  // `the_gap_is_measured_against_the_servers_group_total`; a >50-row fixture
  // costs far more here than it proves.)
  const source = await collectionNamed(request, "Depth Box");
  const view = await viewOf(request, source.summary.id, { limit: 200 });
  const groups = new Map<string, Row[]>();
  for (const r of view.cards.filter((c) => c.present > 0)) {
    const k = `${r.oracle_id}/${r.board}`;
    groups.set(k, [...(groups.get(k) ?? []), r]);
  }
  const pair = [...groups.values()].find((rs) => rs.length >= 2);
  expect(
    pair,
    "dev seed must hold two printings of one card in one collection (build_depth)",
  ).toBeTruthy();
  const [p1, p2] = pair!;

  const scratch = await createCollection(
    request,
    "binder",
    scratchName("group-gap"),
  );
  try {
    // 1 + 2 of the same card, under two printings, in one collection.
    await addHave(request, scratch, p1.printing_id, 1);
    await addHave(request, scratch, p2.printing_id, 2);
    await addWant(request, scratch, p1.oracle_id, 6);

    await expect(async () => {
      const after = await viewOf(request, scratch);
      const rows = after.cards.filter((r) => r.oracle_id === p1.oracle_id);
      expect(rows.length, "two printing rows for one card").toBe(2);
      // The server folds the group's held total onto BOTH rows — the field
      // the client is forbidden from deriving for itself.
      for (const r of rows) {
        expect(r.present_group, "the server folds the group's held").toBe(3);
      }
      // …and the per-row `present` still differs, which is what makes a
      // client-side fold distinguishable from the server's number at all.
      expect(new Set(rows.map((r) => r.present))).toEqual(new Set([1, 2]));
    }).toPass({ timeout: 10_000 });

    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);
    // The cell prints once, on the first of the two rows.
    const cells = page.locator(
      `[data-testid="collection-row"][data-oracle="${p1.oracle_id}"] [data-testid="wanted-count"] [data-testid="count-stepper-value"]`,
    );
    await expect(cells).toHaveCount(1);
    // 6 wanted, 3 held here → 3 still needed. A fold over this row alone would
    // print 5 (6 − 1) or 4 (6 − 2) depending on which printing carried it.
    await expect(cells).toHaveText("3");

    // Stepping to 4 must ask for 3 + 4 = 7. Against a row-local `held` it
    // would have written 5 or 6 — the silent part of the defect.
    const tr = page
      .locator(
        `[data-testid="collection-row"][data-oracle="${p1.oracle_id}"]`,
      )
      .first();
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-inc"]')
      .click();
    await page.locator('[data-testid="collection-title"]').click();
    await expect(cells).toHaveText("4");
    await expect(async () => {
      const v = await viewOf(request, scratch);
      expect(
        v.cards.find((r) => r.oracle_id === p1.oracle_id)!.desired,
      ).toBe(7);
    }).toPass({ timeout: 10_000 });
  } finally {
    await deleteCollection(request, scratch);
  }
});

test("a round trip through grid keeps the session's live numbers, and what they write @fast", async ({
  page,
  request,
}) => {
  // Review round 2, MAJOR. The live `(held, desired)` pair used to be a map of
  // signals built by `CollectionTable`, so its lifetime was that component's
  // MOUNT. Toggling to grid and back rebuilt it from the frozen payload and
  // every session edit silently reverted — and because `held` is a WRITE input
  // (`desired' = held + gap`), the next WANTED commit then saved a number
  // computed from the reverted value. Hoisted to the payload's lifetime, the
  // same one `row_deltas` has.
  const card = await somePrinting(request);
  const scratch = await createCollection(
    request,
    "binder",
    scratchName("grid-roundtrip"),
  );
  try {
    await addHave(request, scratch, card.printing_id, 2);
    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);

    const tr = rowFor(page, card.oracle_id);
    // HERE 2 → 5, in this session only: the payload still says 2.
    await tr.locator(HERE_VALUE).click();
    await tr
      .locator('[data-testid="here-cell"] [data-testid="count-stepper-input"]')
      .fill("5");
    await tr
      .locator('[data-testid="here-cell"] [data-testid="count-stepper-input"]')
      .press("Enter");
    await page.locator('[data-testid="collection-title"]').click();
    await expect(tr.locator(HERE_VALUE)).toHaveText("5");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "5 here",
    );

    // Out to grid and straight back — no reload, no navigation, so `view_res`
    // never refetches and the payload the table rebuilds from still says 2.
    await page.getByRole("radio", { name: "Grid view" }).click();
    await page.waitForURL((url) => url.searchParams.get("view") === "grid");
    await expect(page.getByTestId("collection-grid")).toBeVisible();
    await page.getByRole("radio", { name: "List view" }).click();
    await page.waitForURL((url) => url.searchParams.get("view") !== "grid");
    await hydrated(page);

    // (a) the numbers survived the round trip…
    const back = rowFor(page, card.oracle_id);
    await expect(back.locator(HERE_VALUE)).toHaveText("5");
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "5 here",
    );

    // (b) …and so did what they WRITE. Ask for 5 more than are here: the
    // target must be 5 + 5 = 10. Against a reverted `held` of 2 it would have
    // saved 7 — the silent half of this defect, and the reason the assertion
    // above is not enough on its own.
    await back
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-value"]')
      .click();
    await back
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-input"]')
      .fill("5");
    await back
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-input"]')
      .press("Enter");
    await page.locator('[data-testid="collection-title"]').click();
    await expect(wantedValue(back)).toHaveText("5");
    await expect(async () => {
      const v = await viewOf(request, scratch);
      expect(v.cards[0].present).toBe(5);
      expect(v.cards[0].desired).toBe(10);
    }).toPass({ timeout: 10_000 });
  } finally {
    await deleteCollection(request, scratch);
  }
});

test("a deck section header composes a WANTED edit and a HERE edit in the same row @fast", async ({
  page,
  request,
}) => {
  // Want-stepper review round 1, MAJOR. A section's contribution per card is
  // `max(held, desired)`, and each column's stepper used to compute its own
  // slot delta against a copy of the *other* number captured when it mounted.
  // Held 2 / desired 4: WANTED 4 → 1 correctly pushed −2, and HERE 2 → 3 then
  // pushed `max(3,4) − max(2,4)` = 0 against a `desired` that was already 1,
  // stranding the header at 2 where the truth is 3 — durably, until a reload.
  const card = await somePrinting(request);
  const deck = await createCollection(
    request,
    "deck",
    scratchName("compose"),
    "commander",
  );
  try {
    await addHave(request, deck, card.printing_id, 2);
    await addWant(request, deck, card.oracle_id, 4);
    await page.goto(`/my/collections/${deck}`);
    await hydrated(page);

    // One card in a fresh scratch deck is one section, so its header is
    // unambiguously this row's own contribution: max(2 held, 4 wanted) = 4.
    const section = page.locator('[data-testid="deck-section"]');
    await expect(section).toHaveCount(1);
    await expect(section).toHaveText(/· 4$/);

    const tr = rowFor(page, card.oracle_id);
    // Still needed, not the target: 4 wanted less the 2 already here.
    await expect(wantedValue(tr)).toHaveText("2");

    // Gap 2 → 3, by typing (one action instead of a click each). The target
    // this implies is held + 3 = 5, so the section grows to 5.
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-value"]')
      .click();
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-input"]')
      .fill("3");
    await tr
      .locator('[data-testid="wanted-count"] [data-testid="count-stepper-input"]')
      .press("Enter");
    await page.locator('[data-testid="collection-title"]').click();
    await expect(wantedValue(tr)).toHaveText("3");
    await expect(section).toHaveText(/· 5$/);

    // …then HERE 2 → 3 in the same row, in the same session. Two things have
    // to happen at once, and each was a separate defect: the section header
    // must compute against the want's *post-edit* target (5, so it does not
    // move), and this row's WANTED cell must re-render its gap live, from 3
    // to 2, because the number it shows is `desired − held`.
    await tr
      .locator('[data-testid="here-cell"] [data-testid="count-stepper-inc"]')
      .click();
    await page.locator('[data-testid="collection-title"]').click();
    await expect(
      tr.locator('[data-testid="here-cell"] [data-testid="count-stepper-value"]'),
    ).toHaveText("3");
    await expect(wantedValue(tr)).toHaveText("2");
    await expect(section).toHaveText(/· 5$/);

    // The server agrees, and so does a page rebuilt from it — the live
    // numbers were not merely self-consistent.
    await expect(async () => {
      const after = await viewOf(request, deck);
      expect(after.cards[0].present).toBe(3);
      expect(after.cards[0].desired).toBe(5);
    }).toPass({ timeout: 10_000 });
    await page.reload();
    await hydrated(page);
    await expect(page.locator('[data-testid="deck-section"]')).toHaveText(
      /· 5$/,
    );
    await expect(wantedValue(rowFor(page, card.oracle_id))).toHaveText("2");
  } finally {
    await deleteCollection(request, deck);
  }
});

test("the stepper's last copy is removable, not floored @fast", async ({
  page,
  request,
}) => {
  // This test used to assert the opposite. The floor was `min = 1` because a
  // committed 0 ran `DELETE FROM holdings` while the undo the stepper always
  // offers re-POSTed the dead id — a success toast over vanished copies. It
  // made the destructive commit unreachable, and with no per-row move
  // affordance shipped it made a binder card **impossible to remove**.
  //
  // A committed 0 is now a move with no destination, so what this call site has
  // to keep proving is that the floor did not creep back: at one copy the `−`
  // is live and announced enabled. What the removal *does* — and that its Undo
  // restores the same grain and board — is `removal.spec.ts`.
  const card = await somePrinting(request);
  const scratch = await createCollection(request, "binder", scratchName("floor"));
  try {
    await addHave(request, scratch, card.printing_id, 1);
    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);

    const tr = rowFor(page, card.oracle_id);
    const dec = tr.locator(HERE_DEC);
    await expect(dec).toHaveAttribute("aria-disabled", "false");
    await expect(
      tr.locator(HERE_VALUE),
    ).toHaveAttribute("aria-valuemin", "0");

    // A plain click, not `force`: an `aria-disabled` control would fail
    // Playwright's actionability check, which is what makes this an assertion
    // rather than a hope.
    await dec.click();
    await page.locator('[data-testid="collection-title"]').click();

    await expect(
      page.locator('[data-name="Toast"]', { hasText: "Removed" }),
    ).toBeVisible();
    await expect(async () => {
      expect((await viewOf(request, scratch)).cards).toEqual([]);
    }).toPass({ timeout: 10_000 });
  } finally {
    await deleteCollection(request, scratch);
  }
});

test("emptying a deck moves its cards to the chosen destination", async ({
  page,
  request,
}) => {
  const card = await somePrinting(request);
  const deck = await createCollection(
    request,
    "deck",
    scratchName("teardown-deck"),
    "commander",
  );
  const dest = await createCollection(
    request,
    "binder",
    scratchName("teardown-dest"),
  );
  try {
    await addHave(request, deck, card.printing_id, 2);
    await page.goto(`/my/collections/${deck}`);
    await hydrated(page);

    // Commit a stepper edit *first*, so the teardown below happens with a
    // pending header delta on the page. The delta is zeroed by each new view
    // payload rather than by URL changes; keyed on the URL, the teardown's
    // same-URL refetch left it applied on top of fresh totals and the emptied
    // deck's header read "1 here".
    const tr = rowFor(page, card.oracle_id);
    await tr.locator(HERE_INC).click();
    await page.locator('[data-testid="collection-title"]').click();
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "3 here",
    );
    await expect(async () => {
      expect((await viewOf(request, deck)).totals.present).toBe(3);
    }).toPass({ timeout: 10_000 });

    await page.locator('[data-testid="teardown-open"]').click();
    const picker = page.locator('[data-testid="teardown-destination"]');
    await expect(picker).toBeVisible();
    // The default mode needs no picker; the deck itself is never a destination.
    await expect(
      picker.locator('[data-testid="teardown-previous"]'),
    ).toHaveText("Their previous locations");
    await expect(picker.locator(`option[value="${deck}"]`)).toHaveCount(0);

    await picker.selectOption(dest);
    await page.locator('[data-testid="teardown-confirm"]').click();

    // The deck empties and the destination gains exactly what left.
    await expect(page.locator('[data-testid="collection-empty"]')).toBeVisible();
    // …and the header says so. The refetched totals are 0, and the stepper
    // delta that produced "3 here" a moment ago must not survive them.
    await expect(page.locator('[data-testid="collection-counts"]')).toHaveText(
      "0 here",
    );
    await expect(async () => {
      const emptied = await viewOf(request, deck);
      expect(emptied.cards).toEqual([]);
      expect(emptied.totals.present).toBe(0);
      const landed = await viewOf(request, dest);
      expect(landed.cards.map((r) => r.printing_id)).toEqual([
        card.printing_id,
      ]);
      expect(landed.cards[0].present).toBe(3);
    }).toPass({ timeout: 15_000 });
  } finally {
    await deleteCollection(request, deck);
    await deleteCollection(request, dest);
  }
});

// ------------------------------------------------------------ grid view ----
// The grid/list toggle Catalog already had, applied here
// (WB-01M031Z4MN401FTKNKPE1RZE2E). Table stays the *default* on this route
// (opposite of Catalog's own default — see `collection_url`'s doc comment in
// `app/src/my/collection.rs`), so a bare `/my/collections/:id` load is
// unaffected; `?view=grid` is what opts in.

test.describe("grid view (grid-toggle task)", () => {
  test("the switch renders a grid on a binder, and list stays the default @fast", async ({
    page,
    request,
  }) => {
    const trade = await collectionNamed(request, "Trade Binder");

    await page.goto(`/my/collections/${trade.summary.id}`);
    await hydrated(page);
    await expect(page.getByTestId("collection-table")).toBeVisible();
    await expect(page.getByTestId("collection-grid")).toHaveCount(0);

    const group = page.getByRole("radiogroup", { name: "Result layout" });
    await group.getByRole("radio", { name: "Grid view" }).click();

    await page.waitForURL((url) => url.searchParams.get("view") === "grid");
    await expect(page.getByTestId("collection-grid")).toBeVisible();
    await expect(page.getByTestId("collection-table")).toHaveCount(0);

    // The layout choice is in the URL, so it survives a reload.
    await page.reload();
    await expect(page.getByTestId("collection-grid")).toBeVisible();
  });

  test("a deck's grid keeps its section groupings @fast", async ({
    page,
    request,
  }) => {
    const deck = await collectionNamed(request, "Commander Deck");

    await page.goto(`/my/collections/${deck.summary.id}`);
    await hydrated(page);

    // List mode's own sections — the same locator the plain grouping test
    // above reads, cross-checked against the API there. Read here rather than
    // re-derived, so this test only has to prove the grid *agrees with the
    // table*, not re-implement the bucketing rules a second time.
    const listSections = await page.$$eval("[data-section]", (els) =>
      els
        .map((e) => (e.textContent ?? "").replace(/\s+/g, " ").trim())
        .sort(),
    );
    expect(listSections.length, "dev seed's deck should have sections").toBeGreaterThan(0);

    await page.getByRole("radio", { name: "Grid view" }).click();
    await page.waitForURL((url) => url.searchParams.get("view") === "grid");

    // Only the section heading's own text, not the whole section (which also
    // contains every tile's name, badges and hidden touch-sheet content) —
    // the grid's heading is a sibling `<h3>`, not the `data-section` element's
    // own `textContent` the way the table's single-cell section row is.
    const gridSections = await page.$$eval(
      '[data-testid="deck-grid-section"] > h3',
      (els) => els.map((e) => (e.textContent ?? "").replace(/\s+/g, " ").trim()).sort(),
    );
    expect(gridSections).toEqual(listSections);

    // Sideboard last, same as the table (`app/src/my/collection.rs`'s
    // `BOARD_ORDER`): every mainboard section precedes every "Sideboard · "
    // one in document order.
    const order = await page.$$eval('[data-testid="deck-grid-section"]', (els) =>
      els.map((e) => e.getAttribute("data-section") ?? ""),
    );
    const firstOther = order.findIndex((s) => s.includes(" · "));
    if (firstOther >= 0) {
      expect(order.slice(firstOther).every((s) => s.includes(" · "))).toBe(true);
    }

    // Every card tile still shows the essentials, grouped inside its section.
    const tiles = page.locator('[data-testid="deck-grid-section"] [data-testid="collection-tile"]');
    await expect(tiles.first()).toBeVisible();
  });

  test("child collections render as folder tiles, carrying the sidebar rollup @fast", async ({
    page,
    request,
  }) => {
    const parent = await collectionNamed(request, "Depth Box");
    const rows = await tree(request);
    const kids = rows.filter((r) => r.summary.parent_id === parent.summary.id);
    expect(kids.length, "dev seed should nest a collection under Depth Box").toBeGreaterThan(0);

    await page.goto(`/my/collections/${parent.summary.id}?view=grid`);
    await hydrated(page);
    await expect(page.getByTestId("collection-grid")).toBeVisible();

    for (const kid of kids) {
      const tile = page.locator(
        `[data-testid="folder-tile"][data-collection="${kid.summary.id}"]`,
      );
      await quick(tile).toHaveCount(1);
      await quick(tile.locator("a")).toHaveAttribute(
        "href",
        `/my/collections/${kid.summary.id}`,
      );
      const rolled = rolledUp(rows, kid.summary.id);
      if (rolled > 0) {
        await quick(tile.locator('[data-testid="here-count"]')).toContainText(
          String(rolled),
        );
      }
    }
  });

  test("a grid tile links to the card, shows HERE, and stays selectable @fast", async ({
    page,
    request,
  }) => {
    const trade = await collectionNamed(request, "Trade Binder");
    const view = await viewOf(request, trade.summary.id);
    const held = view.cards.find((r) => r.present > 0);
    test.skip(!held, "dev seed's Trade Binder should hold at least one card");

    await page.goto(`/my/collections/${trade.summary.id}?view=grid`);
    await hydrated(page);
    const tile = page.locator(
      `[data-testid="collection-tile"][data-oracle="${held!.oracle_id}"][data-printing="${held!.printing_id}"]`,
    );
    await expect(tile).toBeVisible();
    await expect(tile.locator("a").first()).toHaveAttribute(
      "href",
      `/cards/${held!.oracle_id}`,
    );
    await expect(tile.getByTestId("here-badge")).toContainText(
      `${held!.present} here`,
    );

    // No stepper on the tile — the count stepper is a list-only editing
    // surface; the grid is a display mode (specs/app-ui.md's Findings entry
    // for the grid-toggle task states this plainly).
    await expect(tile.getByTestId("count-stepper")).toHaveCount(0);

    // Selection stays reachable in grid mode (a deliberate choice — see the
    // same Findings entry): the tile's own select control toggles the shared
    // tray same as a row's `SelectionCheckbox` does.
    const before = page.url();
    await tile.getByTestId("tile-select").click();
    await expect(page.getByTestId("selection-tray")).toBeVisible();
    await expect(page.getByTestId("tray-count")).toContainText("1 card");
    // The tray lives in the shell, not the tile — so on its own, the tray
    // appearing does not prove the click was a *select*, not a navigation
    // that happened to land somewhere the tray also renders. Assert the URL
    // never moved: the click stayed on this page.
    expect(page.url()).toBe(before);
  });

  test("390px: a collection's grid renders without page overflow @fast", async ({
    page,
    request,
  }) => {
    const trade = await collectionNamed(request, "Trade Binder");
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(`/my/collections/${trade.summary.id}?view=grid`);
    await hydrated(page);
    await expect(page.getByTestId("collection-grid")).toBeVisible();
    await expect(page.getByTestId("collection-tile").first()).toBeVisible();

    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow, "the grid should not widen the page at 390px").toBeLessThanOrEqual(1);
  });

  // -------------------------------------------------------- live HERE ----
  // Round-2 review finding: `CollectionBody` freezes the collection's
  // `CollectionView` into a snapshot the moment it resolves, and a stepper
  // commit deliberately never refetches it (the table tolerates this because
  // `HaveStepper` owns its own live displayed value). A `HoldingTile` had
  // nothing of the kind, so a same-session HERE edit was invisible on the
  // tile after switching to grid — stale at best, a ghost tile (rendering
  // for a holding a commit-to-zero just deleted) at worst. Fixed by
  // `RowDeltas` (`app/src/my/collection.rs`): a page-level per-row delta map
  // `HereCount` writes and `CollectionGrid` reads once, on entry.

  test("editing HERE in list mode shows the new count once you switch to grid @fast", async ({
    page,
    request,
  }) => {
    const card = await somePrinting(request);
    const scratch = await createCollection(
      request,
      "binder",
      scratchName("grid-live"),
    );
    try {
      await addHave(request, scratch, card.printing_id, 2);
      await page.goto(`/my/collections/${scratch}`);
      await hydrated(page);

      // Edit while list mode is showing — the only mode with a stepper at all.
      const tr = rowFor(page, card.oracle_id);
      await tr.locator(HERE_INC).click();
      await page.locator('[data-testid="collection-title"]').click();
      await expect(
        tr.locator(HERE_VALUE),
      ).toHaveText("3");
      await expect(
        page.locator('[data-testid="collection-counts"]'),
      ).toHaveText("3 here");

      // Toggle to grid *in the same session*, with no reload and no
      // navigation away — the exact window `view_res` never refetches for.
      await page.getByRole("radio", { name: "Grid view" }).click();
      await page.waitForURL((url) => url.searchParams.get("view") === "grid");

      const tile = page.locator(
        `[data-testid="collection-tile"][data-oracle="${card.oracle_id}"][data-printing="${card.printing_id}"]`,
      );
      await expect(tile).toBeVisible();
      // The new count (3), not the payload's original snapshot (2).
      await expect(tile.getByTestId("here-badge")).toContainText("3 here");
    } finally {
      await deleteCollection(request, scratch);
    }
  });

  test("zeroing a row in list mode leaves no tile for it once you switch to grid @fast", async ({
    page,
    request,
  }) => {
    // Two distinct held printings, not one: emptying the *whole* collection
    // collapses the grid to a genuinely empty (zero-height) container, which
    // would make `toBeVisible()` on the grid itself meaningless. A survivor
    // also doubles as the positive control — proving the fix drops exactly
    // the zeroed row's tile, not the grid's rendering entirely.
    const trade = await collectionNamed(request, "Trade Binder");
    const tradeView = await viewOf(request, trade.summary.id);
    test.skip(
      tradeView.cards.length < 2,
      "dev seed's Trade Binder should hold two distinct printings",
    );
    const zeroed = tradeView.cards[0];
    const survivor = tradeView.cards[1];
    const scratch = await createCollection(
      request,
      "binder",
      scratchName("grid-ghost"),
    );
    try {
      await addHave(request, scratch, zeroed.printing_id, 1);
      await addHave(request, scratch, survivor.printing_id, 2);
      await page.goto(`/my/collections/${scratch}`);
      await hydrated(page);

      // Step the zeroed row's one copy down to zero — a commit-to-zero,
      // which removes the holding server-side (undoable, but not undone
      // here).
      const tr = rowFor(page, zeroed.oracle_id);
      await tr.locator(HERE_DEC).click();
      await page.locator('[data-testid="collection-title"]').click();
      await expect(
        page.locator('[data-name="Toast"]', { hasText: "Removed" }),
      ).toBeVisible();
      await expect(async () => {
        const rows = (await viewOf(request, scratch)).cards;
        expect(rows.some((r) => r.oracle_id === zeroed.oracle_id)).toBe(false);
        expect(rows.some((r) => r.oracle_id === survivor.oracle_id)).toBe(true);
      }).toPass({ timeout: 10_000 });

      // Toggle to grid in the same session. The payload's `view` snapshot
      // still names the zeroed holding (it was never refetched) — the
      // ghost-tile case the fix exists for.
      await page.getByRole("radio", { name: "Grid view" }).click();
      await page.waitForURL((url) => url.searchParams.get("view") === "grid");

      await expect(page.getByTestId("collection-grid")).toBeVisible();
      await expect(
        page.locator(
          `[data-testid="collection-tile"][data-oracle="${zeroed.oracle_id}"][data-printing="${zeroed.printing_id}"]`,
        ),
      ).toHaveCount(0);
      await expect(
        page.locator(
          `[data-testid="collection-tile"][data-oracle="${survivor.oracle_id}"][data-printing="${survivor.printing_id}"]`,
        ),
      ).toBeVisible();
    } finally {
      await deleteCollection(request, scratch);
    }
  });

  // ------------------------------------------------------- live WANTED ----
  // The same two defects, in the column the want-stepper task made editable
  // (its review round 1, MAJOR). `HoldingTile` recomputed its WANTED badge
  // from the frozen payload, and `live_rows` waved every `holding_id: None`
  // row through unconditionally — which used to be sound (such a row had no
  // stepper) and stopped being so the moment a want-only row got one. So a
  // want edited in list mode read its old count in grid, and a zeroed want
  // left a tile badged for a `desires` row that no longer exists. Fixed by
  // extending `RowDeltas` with a `desire_id`-keyed overlay beside the
  // `holding_id` one.

  test("editing WANTED in list mode shows the new count once you switch to grid @fast", async ({
    page,
    request,
  }) => {
    const card = await somePrinting(request);
    const scratch = await createCollection(
      request,
      "binder",
      scratchName("grid-live-want"),
    );
    try {
      await addWant(request, scratch, card.oracle_id, 2);
      await page.goto(`/my/collections/${scratch}`);
      await hydrated(page);

      const tr = rowFor(page, card.oracle_id);
      await tr
        .locator(
          '[data-testid="wanted-count"] [data-testid="count-stepper-inc"]',
        )
        .click();
      await page.locator('[data-testid="collection-title"]').click();
      await expect(wantedValue(tr)).toHaveText("3");

      await page.getByRole("radio", { name: "Grid view" }).click();
      await page.waitForURL((url) => url.searchParams.get("view") === "grid");

      const tile = page.locator(
        `[data-testid="collection-tile"][data-oracle="${card.oracle_id}"]`,
      );
      await expect(tile).toBeVisible();
      // The new count (3), not the payload's original snapshot (2).
      await expect(tile.getByTestId("wanted-badge")).toContainText("3 wanted");
    } finally {
      await deleteCollection(request, scratch);
    }
  });

  test("zeroing a want in list mode leaves no tile for it once you switch to grid @fast", async ({
    page,
    request,
  }) => {
    // A survivor alongside it, for the same reason the HERE ghost test keeps
    // one: an emptied collection collapses the grid to a zero-height
    // container, which would make the absence assertion meaningless.
    const trade = await collectionNamed(request, "Trade Binder");
    const tradeView = await viewOf(request, trade.summary.id);
    test.skip(
      tradeView.cards.length < 2,
      "dev seed's Trade Binder should hold two distinct printings",
    );
    const zeroed = tradeView.cards[0];
    const survivor = tradeView.cards[1];
    const scratch = await createCollection(
      request,
      "binder",
      scratchName("grid-ghost-want"),
    );
    try {
      // Want-only: nothing held, so zeroing the want leaves the row with
      // nothing at all — the case `live_rows` used to wave through.
      await addWant(request, scratch, zeroed.oracle_id, 1);
      await addHave(request, scratch, survivor.printing_id, 2);
      await page.goto(`/my/collections/${scratch}`);
      await hydrated(page);

      const tr = rowFor(page, zeroed.oracle_id);
      await tr
        .locator(
          '[data-testid="wanted-count"] [data-testid="count-stepper-dec"]',
        )
        .click();
      await page.locator('[data-testid="collection-title"]').click();
      await expect(
        page.locator('[data-name="Toast"]', { hasText: "from wants" }),
      ).toBeVisible();
      await expect(async () => {
        const rows = (await viewOf(request, scratch)).cards;
        expect(rows.some((r) => r.oracle_id === zeroed.oracle_id)).toBe(false);
        expect(rows.some((r) => r.oracle_id === survivor.oracle_id)).toBe(true);
      }).toPass({ timeout: 10_000 });

      await page.getByRole("radio", { name: "Grid view" }).click();
      await page.waitForURL((url) => url.searchParams.get("view") === "grid");

      await expect(page.getByTestId("collection-grid")).toBeVisible();
      await expect(
        page.locator(
          `[data-testid="collection-tile"][data-oracle="${zeroed.oracle_id}"]`,
        ),
      ).toHaveCount(0);
      await expect(
        page.locator(
          `[data-testid="collection-tile"][data-oracle="${survivor.oracle_id}"]`,
        ),
      ).toBeVisible();
    } finally {
      await deleteCollection(request, scratch);
    }
  });
});
