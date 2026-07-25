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
  desired: number;
  owned: number;
  present_rollup: number;
  board: "main" | "side" | "maybe";
  holding_id: string | null;
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
/// HERE is read from the count element rather than the cell: an editable cell
/// is a `CountStepper`, whose `textContent` also carries its `−`/`+` button
/// labels. `count-stepper-value` (editable) and `here-count` (not) are the two
/// shapes, and which one appears is itself part of the contract.
async function renderedCells(page: Page) {
  return page.$$eval('[data-testid="collection-row"]', (trs) =>
    trs.map((tr) => {
      const cell = tr.querySelector('[data-testid="here-cell"]');
      const count = cell?.querySelector(
        '[data-testid="count-stepper-value"], [data-testid="here-count"]',
      );
      const rollup = cell?.querySelector('[data-testid="here-rollup"]');
      return {
        oracle: tr.getAttribute("data-oracle") ?? "",
        editable: !!cell?.querySelector('[data-testid="count-stepper"]'),
        here: (count?.textContent?.trim() ?? "") + (rollup?.textContent?.trim() ?? ""),
        wanted:
          tr
            .querySelector('[data-testid="wanted-count"]')
            ?.textContent?.trim() ?? "",
        owned:
          tr.querySelector('[data-testid="owned-count"]')?.textContent?.trim() ??
          "",
      };
    }),
  );
}

/// What a row should render, derived from the API row — the expectation half.
/// Mirrors the spec's rules: WANTED only when set and different, OWNED collapses
/// when equal to the here total, the rolled-up part appended as `+n`.
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
    return {
      oracle: r.oracle_id,
      // Exactly the rows a single `holdings` row backs get the stepper: a cell
      // summing several finish/condition/language grains, or holding nothing,
      // is not addressable by one number.
      editable: r.holding_id !== null,
      here,
      wanted:
        first && r.desired > 0 && r.desired !== r.present
          ? String(r.desired)
          : "—",
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
      await quick(tr.locator('[data-testid="wanted-count"]')).toHaveText(
        String(row.desired),
      );
      // No stepper on a row with nothing here to step.
      await quick(tr.locator('[data-testid="count-stepper"]')).toHaveCount(0);
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

  // A top-level collection goes up to All cards instead.
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
    await tr.locator('[data-testid="count-stepper-inc"]').click();
    await page.locator('[data-testid="collection-title"]').click();

    // Both numbers move, and the *database* moved — asserting only the DOM
    // would pass with the save stubbed out.
    await expect(tr.locator('[data-testid="count-stepper-value"]')).toHaveText(
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
    await expect(tr.locator('[data-testid="count-stepper-value"]')).toHaveText(
      "3",
    );

    // The undo toast reverses it through the same channel.
    const toast = page.locator('[data-name="Toast"]', { hasText: "2 → 3" });
    await expect(toast).toBeVisible();
    await toast.getByRole("button", { name: "Undo" }).click();
    await expect(tr.locator('[data-testid="count-stepper-value"]')).toHaveText(
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

test("the stepper will not commit a count of zero @fast", async ({
  page,
  request,
}) => {
  // The floor is 1, and this is why. `set_holding_quantity(id, 0)` DELETES the
  // holdings row, and the undo the stepper always offers re-POSTs the same,
  // now-deleted id — 404, error toast, copies gone. A success toast with a
  // dead Undo is worse than no zero at all, so `min=1` makes the destructive
  // commit unreachable until the move flows can make it undoable.
  const card = await somePrinting(request);
  const scratch = await createCollection(request, "binder", scratchName("floor"));
  try {
    await addHave(request, scratch, card.printing_id, 1);
    await page.goto(`/my/collections/${scratch}`);
    await hydrated(page);

    const tr = rowFor(page, card.oracle_id);
    const dec = tr.locator('[data-testid="count-stepper-dec"]');
    await expect(dec).toHaveAttribute("aria-disabled", "true");

    // `force`, because Playwright's own actionability check refuses to click an
    // `aria-disabled="true"` control — which is itself the assertion that the
    // floor is announced, not merely enforced. Forcing it dispatches a real
    // click anyway, so the "and it does nothing" half below is genuine.
    await dec.click({ force: true });
    await page.locator('[data-testid="collection-title"]').click();
    await expect(tr.locator('[data-testid="count-stepper-value"]')).toHaveText(
      "1",
    );
    // …and no commit happened, so no toast offered an Undo that could not work.
    await expect(page.locator('[data-name="Toast"]')).toHaveCount(0);

    // …nor by typing a zero into it, which clamps.
    await tr.locator('[data-testid="count-stepper-value"]').click();
    const field = tr.locator('[data-testid="count-stepper-input"]');
    await field.fill("0");
    await field.press("Enter");
    await expect(tr.locator('[data-testid="count-stepper-value"]')).toHaveText(
      "1",
    );

    const after = await viewOf(request, scratch);
    expect(after.cards.length, "the holding must still exist").toBe(1);
    expect(after.cards[0].present).toBe(1);
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
    await tr.locator('[data-testid="count-stepper-inc"]').click();
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
