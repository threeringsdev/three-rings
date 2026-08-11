// Keyset-paging probe for `/my` (specs/app-ui.md → "`/my`"). Not a test — a
// probe, like the hydration and bench checks.
//
// Why this exists as a probe rather than an e2e assertion: the page's 50-row
// page size is fixed, so a browser test walks at most two pages of the dev
// fixture and never sees a boundary condition twice. The paging logic lives in
// `HostedBackend::all_cards`, and the hosted JSON route
// (`GET /api/all-cards?limit=`) is the one caller that can ask for a page small
// enough to iterate. So this walks the whole set at a small limit and asserts
// the five properties keyset paging can get wrong:
//
//   1. no row appears twice across pages (a cursor that is inclusive rather
//      than exclusive repeats the boundary row);
//   2. no row is skipped — the concatenation equals the single-page read,
//      element for element;
//   3. the order is (name, oracle) throughout, across page boundaries;
//   4. `next_cursor` is null exactly once, on the final page;
//   5. every non-terminal page holds exactly the number of rows it ASKED for.
//      This is the only assertion here whose expectation the server does not
//      also compute — an off-by-one in the `limit + 1` / truncate / cursor
//      dance is self-consistent, so 1–4 all still pass under it.
//
// Usage: node all-cards-paging-check.mjs [limit]
//   Needs playwright/.auth/user.json (run `npx playwright test --project=setup`).

import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

// localhost, not 127.0.0.1 — Better Auth's origin check (e2e-suite skill);
// the stored storageState cookie is now scoped to the localhost host.
const BASE = process.env.BASE_URL ?? "http://localhost:3000";
const LIMIT = Number(process.argv[2] ?? 3);

const stateFile = path.join(import.meta.dirname, "playwright/.auth/user.json");
if (!fs.existsSync(stateFile)) {
  console.error(
    `no storageState at ${stateFile} — run \`npx playwright test --project=setup\` first`,
  );
  process.exit(2);
}

let failures = 0;
const check = (ok, label, detail = "") => {
  console.log(
    `${ok ? "ok  " : "FAIL"} ${label}${detail ? ` — ${detail}` : ""}`,
  );
  if (!ok) failures++;
};

const browser = await chromium.launch();
const ctx = await browser.newContext({
  storageState: JSON.parse(fs.readFileSync(stateFile, "utf8")),
  baseURL: BASE,
});

const get = async (url) => {
  const res = await ctx.request.get(url);
  if (!res.ok()) throw new Error(`GET ${url} → ${res.status()}`);
  return res.json();
};

try {
  // The reference: everything in one page. 200 is `Page::limit`'s clamp, so
  // this is the largest single read the API will serve — and asserting it comes
  // back terminal is what makes it a legitimate reference. (The page's own
  // adapter cannot be used for this: it takes no limit, so its 50-row default
  // is itself a page once the fixture exceeds it.)
  const whole = await get("/api/all-cards?limit=200");
  check(
    whole.cards.length > LIMIT,
    "fixture is larger than one probe page",
    `${whole.cards.length} rows at limit ${LIMIT}`,
  );
  check(
    whole.next_cursor === null,
    "the reference read holds the whole fixture",
    whole.next_cursor === null ? "" : "it paged — raise the reference limit",
  );

  // Walk it at LIMIT.
  const walked = [];
  const cursors = [];
  let cursor = null;
  let pages = 0;
  let terminals = 0;
  let wrongSize = 0;
  for (;;) {
    const url =
      `/api/all-cards?limit=${LIMIT}` +
      (cursor ? `&cursor=${encodeURIComponent(cursor)}` : "");
    const page = await get(url);
    pages++;
    // Page size, asserted against the number we ASKED for rather than against
    // anything the server also computed. Every page but the last must be full:
    // an off-by-one in the `limit + 1` fetch / truncate / cursor dance
    // (`HostedBackend::all_cards`) still walks the whole set consistently, so
    // nothing else here notices it — the gap the Codex mutation pass found.
    if (page.next_cursor !== null && page.cards.length !== LIMIT) {
      wrongSize++;
      check(
        false,
        `page ${pages} holds ${LIMIT} rows`,
        `got ${page.cards.length}`,
      );
    }
    walked.push(...page.cards);
    if (page.next_cursor === null) {
      terminals++;
      break;
    }
    if (cursors.includes(page.next_cursor)) {
      check(false, "cursor advanced", `repeated cursor after page ${pages}`);
      break;
    }
    cursors.push(page.next_cursor);
    cursor = page.next_cursor;
    if (pages > 200) {
      check(false, "walk terminates", "over 200 pages — cursor not advancing");
      break;
    }
  }
  check(pages > 1, "the walk really paged", `${pages} pages`);
  check(terminals === 1, "exactly one terminal page", `${terminals}`);
  if (!wrongSize) check(true, `every non-terminal page holds ${LIMIT} rows`);

  // 1. No duplicates.
  const ids = walked.map((r) => r.card.oracle_id);
  const dupes = ids.filter((id, i) => ids.indexOf(id) !== i);
  check(
    dupes.length === 0,
    "no row appears twice",
    dupes.slice(0, 3).join(", "),
  );

  // 2. Nothing skipped — same rows, same order, as the single-page read.
  const same =
    walked.length === whole.cards.length &&
    walked.every((r, i) => r.card.oracle_id === whole.cards[i].card.oracle_id);
  check(
    same,
    "paged walk equals the unpaged read",
    `${walked.length} vs ${whole.cards.length}`,
  );

  // 3. Sorted by (name, oracle) across page boundaries.
  let ordered = true;
  for (let i = 1; i < walked.length; i++) {
    const a = walked[i - 1].card;
    const b = walked[i].card;
    const cmp =
      a.name < b.name
        ? -1
        : a.name > b.name
          ? 1
          : a.oracle_id < b.oracle_id
            ? -1
            : a.oracle_id > b.oracle_id
              ? 1
              : 0;
    if (cmp >= 0) {
      ordered = false;
      check(false, "ordered by (name, oracle)", `${a.name} then ${b.name}`);
      break;
    }
  }
  if (ordered) check(true, "ordered by (name, oracle)");

  // 4. A cursor filters as well as pages: the same walk under a `q` stays
  //    inside the filter (a cursor that dropped `q` would re-widen the set).
  // A needle at least two cards share, derived from the fixture rather than
  // hardcoded (the POC catalog is re-ingestable, the seed re-runnable). Without
  // ≥2 matches the filtered walk has no second page to check.
  const names = whole.cards.map((r) => r.card.name.toLowerCase());
  const needle =
    names
      .flatMap((n) =>
        Array.from({ length: Math.max(0, n.length - 2) }, (_, i) =>
          n.slice(i, i + 3),
        ),
      )
      .find((s) => names.filter((n) => n.includes(s)).length > 1) ??
    whole.cards[0].card.name.slice(0, 4);
  const filtered = await get(
    `/api/all-cards?limit=200&q=${encodeURIComponent(needle)}`,
  );
  const filteredPaged = await get(
    `/api/all-cards?limit=1&q=${encodeURIComponent(needle)}`,
  );
  check(
    filteredPaged.cards.length === 1,
    "a filtered page honors its limit",
    `${filteredPaged.cards.length}`,
  );
  if (filtered.cards.length > 1 && !filteredPaged.next_cursor) {
    // Reachable only when the server is already broken (a mutation that
    // shortens a page can leave a full result set behind a null cursor).
    // Report it rather than throwing on `cursor=null` — a probe that crashes
    // buries the findings above it.
    check(false, "a non-final filtered page carries a cursor", "got null");
  } else if (filtered.cards.length > 1) {
    const second = await get(
      `/api/all-cards?limit=50&q=${encodeURIComponent(needle)}&cursor=${encodeURIComponent(filteredPaged.next_cursor)}`,
    );
    const stillFiltered = second.cards.every((r) =>
      r.card.name.toLowerCase().includes(needle.toLowerCase()),
    );
    check(stillFiltered, "paging past a filtered page keeps the filter");
    check(
      second.cards.length === filtered.cards.length - 1,
      "filtered walk loses exactly the rows it already returned",
      `${second.cards.length} vs ${filtered.cards.length - 1}`,
    );
  } else {
    console.log("skip  filtered-paging (needle matches only one card)");
  }
} catch (e) {
  check(false, "probe ran", String(e));
} finally {
  await ctx.close();
  await browser.close();
}

console.log(failures ? `\n${failures} FAILED` : "\nALL CHECKS PASSED");
process.exit(failures ? 1 : 0);
