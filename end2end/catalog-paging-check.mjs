// Keyset-paging probe for `/catalog` (specs/catalog-search.md → "Result order
// and keyset"). Not a test — a probe, like `probe:paging` for `/my`, whose
// header comment this one mirrors.
//
// Why this exists as a probe rather than an e2e assertion: the page's page
// size is fixed (60, `CATALOG_PAGE_SIZE`, WB-01M033AFA0VSCGB8Z3HTYPFZVD), so a
// browser test walks at most two pages of the dev fixture and never sees a
// boundary condition twice. The endpoint this walks
// (`GET /api/catalog/search?q=&cursor=&limit=`) is the one caller that can ask
// for a page small enough to iterate the whole set — and unlike `/api/all-cards`,
// it needs no session (`/catalog` is public), so this script carries no
// storageState.
//
// P6-043: the only prior coverage of the catalog's own keyset walk was an
// `#[ignore]`d live-DB test (`hosted.rs::search_live`), never run by default.
// This is that coverage, runnable against any dev server.
//
// Two walks:
//   (a) browse-all (`q=`) — cross-checked against `GET /api/catalog/count`,
//       an independent read of the same table with no cursor involved.
//   (b) one filtered query (`q=t:creature`) — `/api/catalog/count` does not
//       filter (it is the anonymous *catalog* size, not a query result size),
//       and the endpoint runs no `COUNT` for a search (specs/catalog-search.md
//       "What a keyset page may claim" — deliberate, it searches as you type).
//       So there is no independent total to check the filtered walk against;
//       it gets the two properties that need no oracle beyond the walk
//       itself: no id repeats, and (name, oracle_id) order holds across every
//       page boundary.
//
// Usage: node catalog-paging-check.mjs [limit]

import { chromium } from "playwright";

// localhost, not 127.0.0.1 — Better Auth's origin check (e2e-suite skill);
// irrelevant to this anonymous endpoint today, but every probe in this
// directory uses the same origin so a future authed assertion here (an
// `owned` cross-check, say) does not have to discover the rule again.
const BASE = process.env.BASE_URL ?? "http://localhost:3000";
// The dev catalog is a full Scryfall ingestion (tens of thousands of rows),
// not the ~3K-printing POC subset the spec's older notes describe — a small
// limit like `probe:paging`'s (3, against `/my`'s ~100-row fixture) would
// need 1,000+ round trips here. 500 still walks 70+ boundary pages while
// finishing in well under a minute.
const LIMIT = Number(process.argv[2] ?? 500);

let failures = 0;
const check = (ok, label, detail = "") => {
  console.log(
    `${ok ? "ok  " : "FAIL"} ${label}${detail ? ` — ${detail}` : ""}`,
  );
  if (!ok) failures++;
};

const browser = await chromium.launch();
const ctx = await browser.newContext({ baseURL: BASE });

const get = async (url) => {
  const res = await ctx.request.get(url);
  if (!res.ok()) throw new Error(`GET ${url} → ${res.status()}`);
  return res.json();
};

/// Walk `/api/catalog/search` at `limit` for the given `q` (empty = browse
/// all), collecting every row across every page until `next_cursor` is null.
/// Returns the concatenated rows and the page count, and fails (via `check`)
/// on a cursor that repeats or a walk that never terminates — the two ways a
/// keyset walk can loop instead of finishing.
async function walk(label, q) {
  const rows = [];
  const cursors = [];
  let cursor = null;
  let pages = 0;
  for (;;) {
    const url =
      `/api/catalog/search?q=${encodeURIComponent(q)}&limit=${LIMIT}` +
      (cursor ? `&cursor=${encodeURIComponent(cursor)}` : "");
    const page = await get(url);
    pages++;
    rows.push(...page.cards);
    if (page.next_cursor === null) break;
    if (cursors.includes(page.next_cursor)) {
      check(false, `${label}: cursor advanced`, `repeated after page ${pages}`);
      break;
    }
    cursors.push(page.next_cursor);
    cursor = page.next_cursor;
    if (pages > 500) {
      check(false, `${label}: walk terminates`, "over 500 pages — cursor not advancing");
      break;
    }
  }
  return { rows, pages };
}

/// No id appears twice across the whole walk — a cursor that is inclusive
/// rather than exclusive of its own boundary row repeats it on the next page.
function assertNoDupes(label, rows) {
  const ids = rows.map((r) => r.oracle_id);
  const seen = new Set();
  const dupes = [];
  for (const id of ids) {
    if (seen.has(id)) dupes.push(id);
    seen.add(id);
  }
  check(dupes.length === 0, `${label}: no id appears twice`, dupes.slice(0, 3).join(", "));
}

/// (name, oracle_id) strictly increases across every page boundary, not just
/// within a page — the property a per-page-only check would miss entirely.
function assertOrdered(label, rows) {
  let ordered = true;
  for (let i = 1; i < rows.length; i++) {
    const a = rows[i - 1];
    const b = rows[i];
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
      check(false, `${label}: ordered by (name, oracle_id)`, `${a.name} then ${b.name}`);
      break;
    }
  }
  if (ordered) check(true, `${label}: ordered by (name, oracle_id)`);
}

try {
  // (a) Browse-all, cross-checked against the independent count endpoint.
  const browseCount = (await get("/api/catalog/count")).cards;
  const { rows: browseRows, pages: browsePages } = await walk("browse-all", "");
  console.log(
    `\nbrowse-all: walked ${browsePages} pages, ${browseRows.length} rows, at limit ${LIMIT}`,
  );
  check(
    browsePages > 1,
    "browse-all: the walk really paged",
    `${browsePages} pages (raise the fixture or lower the limit if this is 1)`,
  );
  assertNoDupes("browse-all", browseRows);
  assertOrdered("browse-all", browseRows);
  check(
    browseRows.length === browseCount,
    "browse-all: walked row count matches GET /api/catalog/count",
    `${browseRows.length} walked vs ${browseCount} counted`,
  );

  // (b) One filtered query. No independent count endpoint exists for an
  // arbitrary query (see header) — falling back to no-dups + monotonic order,
  // stated here rather than silently doing less than (a).
  const FILTERED_Q = "t:creature";
  const { rows: filteredRows, pages: filteredPages } = await walk(
    "filtered (t:creature)",
    FILTERED_Q,
  );
  console.log(
    `\nfiltered "${FILTERED_Q}": walked ${filteredPages} pages, ${filteredRows.length} rows, at limit ${LIMIT}`,
  );
  console.log(
    `note: no independent count for a filtered query (specs/catalog-search.md — the`,
  );
  console.log(
    `endpoint runs no COUNT); asserting no-dups + monotonic keyset order only.`,
  );
  check(
    filteredRows.length > 0,
    "filtered: the query matched something",
    filteredRows.length > 0
      ? `${filteredRows.length} rows`
      : `0 rows for "${FILTERED_Q}" — pick a broader probe term`,
  );
  assertNoDupes("filtered", filteredRows);
  assertOrdered("filtered", filteredRows);
} catch (e) {
  check(false, "probe ran", String(e));
} finally {
  await ctx.close();
  await browser.close();
}

console.log(failures ? `\n${failures} FAILED` : "\nALL CHECKS PASSED");
process.exit(failures ? 1 : 0);
