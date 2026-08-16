# Catalog search

**Status:** implemented
**Depends on:** [ui-design](ui-design.md), [data-model](data-model.md)

The queryable field vocabulary and the base search indexes come from
[data-model](data-model.md) — which says *"the query→SQL translation is
catalog-search's"* and *"Data-model provides the base; catalog-search
refines"*; the two-surface UX and the rail's curated vocabulary come from
[ui-design](ui-design.md). **[catalog-ingestion](catalog-ingestion.md) is a
runtime sibling, not a design dependency** — its POC subset (implemented
2026-07-16, ~3K printings on dev) is what this spec's engine develops against.
[collection-api](collection-api.md)'s search endpoint executes this query
grammar; its backend is settled as **SQL against our ingested catalog**, and
the endpoint shell already exists (`POST /api/catalog/search`,
`SearchQuery { q } + Page → SearchResults { cards, next_cursor }`, a
trgm-backed name ILIKE until this spec replaces the WHERE clause).

## Problem

Catalog mode's search has two input surfaces — a query bar and a filter rail —
and their relationship needs a defined contract. The catalog dataset comes from
Scryfall, so users will arrive knowing Scryfall's query syntax
(`t:instant c:ur cmc<=2`); the query bar should honor as much of it as
practical. The rail, by contrast, is deliberately a curated everyday subset
(name, text, set, color, type, rarity, mana value — per the Phase 1b
wireframes), not a reproduction of Scryfall's full advanced-search form.

## Scope

**In:** the v1 query-syntax subset (exact vocabulary below); the parser and
its query→SQL translation (replacing the shell's WHERE clause); the search
indexes/columns the translation needs (one migration, per data-model's
delegation); the query↔rail sync contract; URL serialization; result order +
keyset compatibility; error behavior for unsupported syntax; the live-typing
debounce budget.

**Out:** the ingestion pipeline (catalog-ingestion); the endpoint plumbing,
DTOs, and pagination mechanics (collection-api — implemented); the rail's
visual implementation (rides the UI phase); relevance ranking and an `order:`
parameter (deferred — see Decisions); full boolean grammar (`or`, parens).

## Design

### One filter state, two views over it

- The rail and the query bar both edit a single underlying search state; **the
  query text is the canonical serialization** of that state and is what goes
  in the URL (`/catalog?q=…&cursor=…` — shareable/restorable, SSR-able).
- Rail edits rewrite their corresponding term in the query text (checking
  Blue+Red keeps exactly one `c:` term in sync); bare name words map to the
  rail's name field, `o:` to its card-text field.
- Query-bar terms the rail understands reflect back into rail state
  (checkboxes, badges — the mobile filter-sheet badge counts rail-matched
  terms, per the wireframes).
- Editing either surface never destroys the other's state: unrecognized-by-
  the-rail terms (e.g. `id:`, negations) simply don't appear in the rail and
  are preserved verbatim on rail edits.

### V1 syntax subset

A **flat AND of terms** — no `or`, no parentheses (that's a real grammar;
deferred). Terms are whitespace-separated; quotes group phrases; `-` prefixes
negate any term. One deliberate micro-extension: **comma = OR within one
term's values** (`r:rare,mythic`, `s:mh3,lea`) — it's what the rail's
multi-select facets serialize to, since flat Scryfall syntax cannot express
"rare OR mythic" without parens. Plain Scryfall habits still parse.

| term | matches | notes |
|---|---|---|
| bare word / `"a phrase"` | name substring | multiple words AND (Scryfall behavior) |
| `name:` | name substring | explicit form of the above |
| `o:` `oracle:` `text:` | oracle-text substring | searches top-level text **and every face** (multi-face concat — the data-model note) |
| `t:` `type:` | type_line substring | combined type_line covers both faces |
| `s:` `set:` `e:` | set code, comma-OR | printing-scoped |
| `r:` `rarity:` | rarity equality, comma-OR | printing-scoped |
| `c:` `color:` | card has all listed colors | per Scryfall "any face qualifies"; `c:colorless` supported |
| `id:` `identity:` | color identity **within** listed | commander semantics (`⊆`) |
| `mv:` `cmc:` (also `=` `<` `<=` `>` `>=`) | mana value compare | `mv:3`, `mv<=2`, `cmc>4` |

Anything else — unknown keys (`pow>3`, `is:commander`, `f:modern`), `or`,
parens — is a **parse error naming the offending term**, surfaced as a
validation error to the UI (never silently-wrong results; Scryfall itself
errors on unknown terms). The vocabulary grows term-by-term later; `f:`
(legalities) and `is:` flags are the obvious next additions since the columns
already exist.

### Parser

A hand-rolled tokenizer/parser (no grammar dependency): split on whitespace
respecting quotes → per token read optional `-`, optional `key` + operator
(`:` `=` `<` `<=` `>` `>=`), comma-split values → `Vec<Term>` AST. Pure
function, unit-tested (TDD), lives beside the translation in `app` (behind
`hosted` — wasm never needs it in v1; the rail's term↔widget mapping is a
UI-phase concern and may motivate moving the *parser* (not the SQL) to
`shared/` then).

### Query → SQL

The translation builds one WHERE clause of ANDed predicates over the shell's
existing keyset query — **bind parameters only**, never string-spliced values.
Results stay **oracle-grain** (`CardSummary`); printing-scoped terms decide
which oracles qualify.

- **name** — `c.name ILIKE '%' || $n || '%'` per word/phrase
  (`cards_name_trgm_idx` GIN serves it).
- **`o:`** — same ILIKE against a new **generated column**
  `cards.oracle_search_text` = lower(top-level `oracle_text` + every
  `card_faces[*].oracle_text`), with its own trgm GIN index. Substring
  semantics match `name` and avoid tsvector stemming surprises ("blocks" ≠
  "block" in rules text); a tsvector is the later relevance upgrade path.
- **`t:`** — `c.type_line ILIKE …` (+ trgm GIN on `type_line`).
- **`c:`** — `c.colors @> $arr OR c.card_faces @> $probe` — the jsonb
  containment probe (`[{"colors":["U","R"]}]`) implements "any single face
  has them all" for multi-face cards, whose top-level `colors` is empty by
  design; `c:colorless` = `colors = '{}' AND card_faces IS NULL` (single-face)
  or all faces colorless.
- **`id:`** — `c.color_identity <@ $arr` (identity is whole-card top-level).
- **`mv:`** — `c.cmc <op> $n` (top-level even on multi-face; NULL on
  reversible ⇒ never matches, correct).
- **`s:` / `r:`** — **all positive printing-scoped terms share one
  `EXISTS (SELECT 1 FROM printings p JOIN sets … WHERE p.oracle_id =
  c.oracle_id AND …)`** so `s:mh3 r:common` means one printing satisfying
  both (Scryfall semantics), not two different printings. Each **negated**
  printing-scoped term is its own `NOT EXISTS`.
- **negation** — `NOT (…)` around the term's predicate.
- **empty query** — browse-all, name-sorted (valid and useful).

**Migration (this spec's, per data-model's delegation):** the
`oracle_search_text` generated column + its trgm GIN index, and trgm GIN on
`type_line`. GIN on `colors`/`card_faces` deferred until profiling says the
seq-scan residue matters (most queries carry a name/type/text term that
already narrows via index).

### Result order and keyset (a deviation to record)

collection-api fixed the search sort key as *"relevance, then name"* — but
relevance ranking is hostile to keyset cursors (it isn't a stable column), and
**Scryfall's own default sort is name ascending**, which is what users expect
from it anyway. **V1 orders by `(name, oracle_id)`** — exactly the shell's
existing cursor — and defers relevance to a future `order:`/ranking extension
(the tsvector upgrade path). Recorded as a correction note in collection-api
at acceptance.

### Live typing

Both surfaces promise live results (wireframes). Proposal: **~250 ms
debounce** after the last keystroke, one in-flight request with stale-response
discard (monotonic request ids), first page SSR-rendered when the URL carries
`q`. Numbers tunable at execution; the contract is "no stale results ever
render over newer input".

## Findings (implementation — 2026-07-16)

Shipped as `app/src/search/` — `parse.rs` (the pure grammar, dependency-free)
+ `sql.rs` (QueryBuilder emission, binds only) — wired into
`HostedBackend::search` replacing the shell's WHERE; parse errors surface as
`ApiError::Validation` (422) carrying the term-naming message. 25 unit tests
(TDD) plus an `#[ignore]`d end-to-end test against the live dev POC catalog
(`DATABASE_URL=… cargo test -p app --features hosted -- --ignored
query_engine`), which verified: keyset browse-all paging; name substring;
**back-face oracle text** (a phrase existing only on Ral, Leyline Prodigy — a
transform back face — found via the generated column); combined card-scoped
terms; printing-scoped comma-OR under one EXISTS; colorless + identity on
Alpha artifacts; negation; and the 422 naming `pow>3`.

- **Migration `0008` applied to dev:** the `oracle_search_text` generated
  column (`jsonb_path_query_array` over `card_faces` — confirmed IMMUTABLE
  live, so the generated column is legal) + trgm GINs on it and `type_line`.
  Additive; prod rides the same `migrate.sh prod` as `0007` at merge.
- **Bug caught by the live test, not the unit tests:** `card_faces IS NULL`
  on single-face cards made the color-probe predicate evaluate to SQL NULL,
  and `NOT (false OR NULL)` is NULL — so negated color terms silently dropped
  every single-face row. Fixed with `coalesce(card_faces,'[]')`; a unit test
  now locks the coalesce in place. (Positive terms had worked only because
  NULL is falsy in WHERE.)
- Grain note: the run-stat "2,665 cards" in catalog-ingestion counts *writes*
  (including pre-first-seen-fix flips); the table holds **2,637 distinct
  oracles**.
- **Rail sync + URL serialization ship as contract, not code**: the query
  string is the canonical state and the endpoint consumes it; the rail's
  term↔widget mapping is implemented by the UI-phase catalog screen tasks
  against this spec's contract.

## Decisions (this review)

- **Flat AND-of-terms v1** with per-term `-` negation and the **comma-OR
  micro-extension** for rail multi-selects; no `or`/parens (real grammar,
  deferred). Unknown syntax **errors, naming the term** — never
  silently-wrong results.
- **Substring semantics everywhere** (trgm ILIKE) — name, oracle text, type
  line behave identically; `oracle_search_text` generated column solves the
  multi-face `o:` gap flagged by data-model's shape review; tsvector deferred
  as the relevance upgrade.
- **Printing-scoped terms share one EXISTS** (Scryfall's per-printing
  semantics); negated ones get their own NOT EXISTS.
- **Order = (name, oracle_id)**, matching Scryfall's default sort and the
  shell's keyset; the "relevance, then name" line in collection-api is
  corrected by note at acceptance.
- **Rail indicator dropped for v1** — with the v1 vocabulary nearly every
  term is rail-representable; non-representable ones (negations, `id:`) are
  preserved-but-invisible, and the mobile badge already counts rail-matched
  terms. Revisit if silence confuses in practice.

### Deviation: `t:` gained comma-OR (2026-07-19, the filter-rail task)

The V1 table above marks comma-OR on `s:` and `r:` only. The rail's Type facet
is a **multi-select** (wireframes: Creature/Instant/Sorcery/Artifact/
Enchantment checkboxes), and this spec's own rationale for the comma extension
— "flat Scryfall syntax cannot express *rare OR mythic* without parens, and it
is what the rail's multi-select facets serialize to" — applies to it verbatim.
`s:`/`r:` were simply the facets that existed when the grammar shipped.

So `Pred::TypeLine` now carries `Vec<String>` and `t:instant,sorcery` means
either. SQL is `c.type_line ILIKE ANY($1)` — one bind, and **parenthesized**,
because `apply` splices predicates in after a bare `AND` and an unparenthesized
disjunction would out-scope the surrounding ANDs and silently widen the query.
Negation still wraps the whole group, so `-t:a,b` is "neither".

Colors deliberately did **not** get this: `c:` means "has all of these", so its
values are one letter-set (`c:ur`), not an OR list.

### Comma-OR values dedupe (2026-08-12, P6-139)

A comma-OR list is a **set**, not a sequence — `s:mh3,lea,mh3` means the same
thing as `s:mh3,lea`. The parser now dedupes it that way: order-preserving,
first occurrence wins, case-insensitively (values are already lowercased
before the comparison). This applies to every comma-OR facet (`s:`/`set:`/
`e:`, `r:`/`rarity:`, `t:`/`type:`) since all three are set-semantics the same
way `id:`'s letter-set already was (`color_letters` has deduped since it
shipped; `csv` — the comma-list counterpart — simply hadn't caught up).

Without this, a hand-typed `s:mh3,mh3` parsed to two identical `Set` values,
and the rail rendered one chip per value — two chips sharing one
`data-testid`/`data-code`, a Playwright strict-mode landmine, and confusing on
screen besides (removing one chip dropped the badge count but left the picker
row's ✓ showing, since the other duplicate was still selected). The picker's
own `toggle_code` got the matching fix: it now lowercases before the
membership check, not just before the push, so it can't append a
differently-cased duplicate of a code already selected either.

### What a keyset page may claim about the result set (2026-08-12)

"Result order and keyset" settles the *order*; this settles what the UI is
allowed to say about a page of it, which the paging-honesty batch (P6-130…133,
app-ui Findings) had to decide.

The endpoint runs no `COUNT` and a keyset cursor carries no offset, so a page
knows its own rows and nothing else. The rendering follows exactly that:

- **page one, no `next_cursor`** — `23 results`. The page *is* the result set.
- **page one, with `next_cursor`** — `50+ results`. "At least 50", the reading
  already recorded for the first-page count.
- **past a cursor** — `50 results on this page`. The rows before the cursor are
  not counted and cannot be, so a bare "50 results" is false and "50+" is worse
  (it reads as a claim about the total). No `+` here even when more follows: the
  page holds exactly what it holds, and the qualifier already refuses the total.

Rejected: a `COUNT` query (this box searches as you type — it would run per
keystroke) and a page ordinal in the URL (a new parameter every writer of a
catalog URL must thread, kept in sync with a cursor that can arrive from a
shared link with no ordinal beside it). Both remain available if a real total is
ever wanted; neither is needed to stop lying.

Two adjacent rules the same batch settled, recorded here because they are
properties of the cursor rather than of the screen: **a cursor is only ever
actionable for its own query** — while the results on screen answer a query the
box no longer holds, the pager is inert rather than pointing back at the old
pair — and **a cursored page is never retained as "previous results"** under a
later parse error, since it names a position in a search nobody is editing any
more.

### A corrupt `?cursor=` is not a claim about the query (2026-08-13, P6-043)

Before this task, `HostedBackend::search`'s `decode_cursor` failed a bad
base64/JSON `?cursor=` as `ApiError::Validation("invalid cursor")` — the exact
variant a rejected grammar term also produces. `/catalog`'s error arm rendered
any `Validation` as the grammar's own words, no "Search failed" prefix (see
"Parse errors are results, not failures" above), so a syntactically fine query
sitting behind a stale or hand-edited cursor read as if the *query* — "bolt",
in the concrete e2e case — had been rejected. It had not; only the page
reference had.

Fix: `ApiError` gained a distinct `BadCursor` variant (shared/src/lib.rs),
mapped to the same 422 as `Validation` — the two 422s are told apart on the
wire by the error envelope's `code` (`"bad_cursor"` vs `"validation"`), which
`ApiError::from_wire` now reads before falling back to the status-implied
table. `decode_cursor` returns `BadCursor` instead of `Validation`.
`/catalog`'s error arm (`app/src/catalog.rs::describe_error` /
`QueryErrorKind`) now renders it as its own case — "This page link is no
longer valid." — rather than echoing "invalid cursor" as a verdict on the
box, while keeping the escape hatch this same batch already built: the
"← Back to the start" link drops only the cursor, re-running the query that
was never at fault. `components::states::Failure::of_api_error` groups
`BadCursor` with `Validation` under `Failure::Request` (not retryable, same
"way out" affordance contract) — the query-vs-cursor distinction is a
wording decision `/catalog`'s own error arm makes, not a fourth `Failure`
class.

Also from this task: `probe:catalog-paging` (`end2end/catalog-paging-check.mjs`)
is the catalog's counterpart to `probe:paging` — the only prior coverage of
the catalog's own keyset walk was an `#[ignore]`d live-DB test
(`hosted.rs::search_live`), never run by default. It walks browse-all
end to end via `GET /api/catalog/search`, cross-checked against the
independent `GET /api/catalog/count`, and walks one filtered query
(`t:creature`) asserting no-dup-ids + monotonic `(name, oracle_id)` order —
the filtered walk has no independent count to check against, since the
endpoint runs no `COUNT` for a search (previous section). Against the dev
catalog (38,623 rows) at `limit=500`: browse-all walked 194 pages / 38,623
rows exactly matching the count, zero duplicates; the filtered walk covered
101 pages / 20,087 rows, zero duplicates, order held throughout both.

### Numbered page links (2026-08-15, WB-01M032Q6BX8BM7NPK8H3AQKGWF)

`/catalog`'s pager grew a numbered strip (`[Prev] [1] … 9 [10] [11] [12] … [28]
[Next]`, up to 6 numbers, current plain-text) replacing the old right-aligned
"Next page" / "Back to the start" pair. The pure shape is `page_strip(current,
last: Option<usize>) -> Vec<PageSlot>` (`app/src/catalog.rs`), unit-tested
against the task's three worked examples verbatim plus 1/2/6/7-page and
`current == last` edges.

**This reverses "Rejected: … a page ordinal in the URL" above** (2026-08-12
paging-honesty batch) — that batch's reasons (URL-thread cost, syncing an
ordinal with a cursor that can arrive from a shared link with no ordinal
beside it) no longer apply once the ordinal's job is purely cosmetic: `?page=`
now rides beside `?cursor=` as a **display-only label** (`PAGE_PARAM`), echoed
through `SearchPayload` the same way `q`/`cursor` are, never read by
`results`' fetch key. A stale or hand-edited `?page=` can only mislabel the
number shown, never fetch the wrong rows — the correctness property the old
rejection was protecting stays intact.

**Which numbers are real links is bounded by what forward-only keyset paging
can actually address.** Confirmed against real dev data (38,623 rows / 773
pages at the current 50-row page size, derived from the observed page length —
`page_size` is nowhere hardcoded, so the queued 50→60 page-size task needs no
edit here): only three kinds of page number are ever backed by a cursor this
screen has — page 1 (the empty cursor), the current page, and current + 1
(`next_cursor`, identical to what Next already uses). A client-side `trail`
(`CatalogPage`, one entry per page a reader has actually stood on this search)
adds every page reached by paging forward, which is what makes **Prev** — and
a band member behind `current`, in the "counting down near the end" shape —
real navigations too, without a reverse-ordered query. Any other number
`page_strip` wants to show (a page ahead nobody has walked to yet, or
browse-all's true last page before it's been reached) renders **inert**: a
real `<a aria-disabled="true">`, not a fake link and not omitted. Verified live
(screenshots + Playwright, not just the pure function): page one shows
`[Prev✗] 1 [2] [3✗] [4✗] [5✗] … [773✗] [Next]`; walking forward via real clicks
makes Prev and page one real at every step; a cold load 8 hops into a filtered
search (no trail) shows only `[1] … 9 [10]`, matching the pure function's
`last = None` degrade exactly.

**`last = None` is a third rendering mode**, not just a missing-data
fallback: a filtered search never gets a total until it is *walked* to its
real end (`next_cursor` comes back empty — knowable with no `COUNT`, same as
the existing count-label honesty rule above), so `page_strip` degrades to
naming only 1, current, and current + 1 rather than fabricate a "last" this
screen cannot back up. Direct page-N addressing (jumping to an unwalked page
without a cursor) needs either an offset-capable query or a server-side walk —
genuinely out of scope here (a materially different, larger change than a
pagination UI), flagged as a follow-up rather than filed by this task.

**A live-only bug this batch found and fixed, worth recording because it
would bite the next dynamic list of links inside a `<Transition>`:** building
each numbered link as its own child component taking `href:
Signal<Option<String>>` via `Signal::derive(move || …)` — the same shape the
pre-existing "Back to the start" / "Next page" links used — panicked at
runtime ("you tried to access a reactive value … but it has already been
disposed") the instant a sibling signal (`stale` flipping on a keystroke, or a
`trail` write landing in the same reactive tick as the old pager's teardown)
changed while `<Transition>` still held the previous pager mounted. Unit tests
and `cargo check` are both silent on this — it only surfaces as a live WASM
panic that kills all further reactivity on the page, caught here only by
driving the real dev server with Playwright (`page.on("pageerror", …)`), not
by `cargo test` or a cold curl of the SSR HTML. The fix: build the whole strip
as **one** `{move || { … }}` dynamic child (the same pattern
`ResultCards` already uses for `list_view`), rather than N sibling components
each owning an independent derived signal. No repro was reduced to a minimal
case beyond this one; flagged here so a future `Signal::derive`-per-list-item
pattern near a `Transition` gets tested against a real browser, not just
`cargo test`.

### Numbered page links, round 2: cursor-less jumps (2026-08-15, maintainer ruling, WB-01M032Q6BX8BM7NPK8H3AQKGWF)

**Supersedes the section above's "inert, present-but-unreachable" compromise
in full: "this PR does not merge until every rendered page number is a REAL
link from day one."** The forward-only-keyset limitation that produced that
compromise is real, but the fix is a second, narrower paging primitive
alongside the keyset one, not a permanent constraint on the pager.

**The mechanism — explicit jumps get an `OFFSET`, typing still doesn't.**
`CatalogStore::search` gained a third argument, `page_number: Option<u32>`
(`app/src/backend/mod.rs`), independent of `Page`
(`shared::collection::Page`, also `/my`'s type — deliberately not touched, to
keep this a catalog-search-only capability). When set, `hosted.rs` computes
`OFFSET (page_number - 1) * limit` under the *same* `ORDER BY (name,
oracle_id)` the keyset cursor uses (`page_offset`, `hosted.rs`, its own unit
tests) instead of the `cursor`'s `AND (name, oracle_id) > (...)` clause — a
`page_number` wins if both are present, though this app's own UI never
generates both at once. **The per-keystroke path is untouched**: typing (or a
rail edit, or Enter) always drops both cursor and page and re-fetches page one
plain, exactly the query it always was — this only ever engages on an explicit
pager click. `?page=` in the URL, cosmetic-only in round 1, is now the real
fetch input when no `?cursor=` rides beside it (`results`' key in
`CatalogPage`); a `?cursor=` still wins when both are present, so a legacy
shared link from before this ruling keeps working unchanged.

**The trail is gone.** With every page directly addressable, remembering
which pages a reader had actually visited stopped earning its keep — deleted
along with its `RwSignal<HashMap<usize, String>>` and the `Effect` that grew
it. `Pager`'s `href_for` is now a single, un-conditional `catalog_url(q, view,
None, Some(n))` for every slot the strip shows, including Prev.

**A real total for filtered queries: `search_count`, a second, independent
query.** `CatalogStore::search_count(query) -> CatalogCount` (`hosted.rs`)
runs `SELECT count(*) FROM cards c WHERE true` plus the *same*
`crate::search::sql::apply(&mut qb, &terms)` `search` itself uses — never
folded into `search`'s own query, never gating its response. Wired through a
sibling Leptos server fn (`search_catalog_count`, GET, same shape as
`search_catalog`) and a sibling JSON route (`GET
/api/catalog/search/count?q=`). `CatalogPage` fires it as its own `Resource`,
keyed the same way `results` is (once per settled query — the existing
debounce is the only throttle it needs, since it never runs more often than
`results` itself does) but **never awaited alongside `results`**: `Pager`
reads it with a plain `.get()` inside its one reactive dynamic child, so the
strip renders with whatever `results` already has and *upgrades in place* the
moment the count resolves — confirmed live (Playwright, holding the count
route open): typing into a filtered query shows the near band and the
*previous* query's (now-stale) last page immediately, then swaps to the new
query's own true last page once its count lands, with zero console errors
through the whole sequence.

**`page_strip`'s two-tier honesty collapses to one common case now.** `last =
Some(page)` when `next_cursor` is empty (this genuinely is the last page,
still free) or once `search_count` resolves (now: *any* query, not just
browse-all). `last = None` persists only for the brief window before
`search_count` has resolved at all — no longer the permanent state a filtered,
not-yet-finished search was stuck in through round 1.

**Adversarial-review blocker, fixed: unchecked `usize`/`u32` arithmetic on a
client-suppliable page number.** `GET /catalog?page=18446744073709551615`
(`usize::MAX` on a 64-bit build) reached unguarded `page + 1` (`Pager`) and
`current + 1`/`current + 3` (`page_strip`) and panicked an anonymous SSR
request in debug builds. Two independent layers now, neither trusting the
other:

1. **Parse-time ceiling** — `parse_page` (`app/src/catalog.rs`) clamps
   `?page=` to `1..=MAX_PAGE` (`1_000_000`, chosen only to be far past any
   plausible catalog size while keeping every downstream `usize`/`u32`
   computation comfortably bounded), covering absent/zero/negative/unparsable
   too. Unit-tested against the crafted string verbatim.
2. **`saturating` arithmetic in `page_strip` and `page_offset`**, independent
   of what any caller clamped — `page_strip(usize::MAX, …)` and
   `page_offset(u32::MAX, 200)` are both unit-tested directly and neither
   panics nor wraps.

**Resolution for an out-of-range page (past the true last one): the honest
empty page, not a redirect or a clamp.** `OFFSET` past the end of a Postgres
result set is not an error — it returns zero rows, which the *existing*
`NoResults`/"Nothing on this page. Back to the start." UI already renders
correctly (round 1: "past a cursor... the reader has walked off the end").
Confirmed live for both the client's own clamped ceiling
(`/catalog?page=18446744073709551615` → 200, "Nothing on this page") and a
raw hosted-API call bypassing the client's clamp entirely
(`/api/catalog/search?page=4294967295` → 200, `{"cards":[],"next_cursor":null}`)
— no new code needed either way.

**Offset cost, measured honestly (the maintainer asked for this specifically,
expecting a real number, not an assumption).** Against the dev catalog
(38,623 rows) over the network to Neon's dev branch (not local Postgres):
page 1 (no offset) ~160–210 ms; page 700 (`OFFSET ≈34,950`) ~425–570 ms; page
773, the true last page (`OFFSET ≈38,600`) ~480–540 ms. `search_count`
(a full `count(*)`) ~90 ms unfiltered, ~145–155 ms filtered. So: a real,
measurable cost — roughly 2–3× a keyset fetch at this catalog's current
size — but still comfortably sub-second, and paid only on an explicit pager
click, never on a keystroke. Not benchmarked at a materially larger catalog
size; if ingestion grows the row count by an order of magnitude or more,
re-measure before assuming this still holds (`OFFSET` cost is roughly linear
in the skipped-row count on an indexed sort, so the trend is predictable even
without a fresh measurement, but "predictable" is not "measured").

Verified live end to end, not just the pure function and its callers in
isolation: a **fresh** `/catalog?page=9` request — no prior visit, no
cookies, a single cold HTTP request — SSRs the real ninth page with every
pager number (Prev, 1, 8, 10, 11, 12, the true last page, Next) a working
link and zero `aria-disabled` anywhere on the strip; clicking the true last
page directly from page one (no intermediate clicks) lands there in one
navigation.

### Filtered header counts, closing the loop (2026-08-15, WB-01M0324HQ12B590CZ0YXJPB5T6)

The "N cards in the catalog." line under the `Catalog` header only ever
rendered for browse-all — as soon as a query filtered the results, the line
disappeared with no replacement. This was pure render work: `search_count`
above already supplies the exact total for *any* query, filtered or not, and
`CatalogPage` was already firing it unconditionally. No new resource, no new
server fn, no new route — the header just wasn't reading the one that
existed.

**Wording:** the unfiltered sentence is untouched. Filtered gets its own,
`"{n} cards match."` — no `+` qualifier the way `count_label`'s per-page
phrase needs one, because `search_count` is a real `count(*)`, exact
regardless of how many pages the search runs to (`filtered_count_label`,
`app/src/catalog.rs`, unit-tested).

**Zero results are silent, not `"0 cards match."`** `NoResults` already
renders "No cards match that search." in the body for the zero case; the
header repeating the same verdict a second time above the grid would be the
same fact stated twice, not two facts.

**Pending/staleness: a second `<Transition>`, deliberately its own boundary,
plus `Pager`'s own dimming — not just "keep the old label."** The filtered
line lives in its own `<Transition fallback=|| ()>` — reserving nothing
before the first resolve (matching the unfiltered line's existing behaviour)
and, on every later query change, keeping the *previous* query's label on
screen until the new one lands rather than collapsing to blank. Kept as its
**own** `<Transition>`, separate from `Results`': awaiting `search_count`
here never gates the cards or the pager on this line's slower, independent
request.

That much alone is **not** a safe mirror of `Pager`'s staleness story,
though an earlier revision of this section claimed it was: `results` and
`search_count` are two independent round trips of *similar* latency
(measured above: ~90–155 ms unfiltered/filtered for the count, a comparable
order for a keyset page), so either can resolve first. When `results` wins
— edit a 40-hit query to a 0-hit one, and the cards settle into `NoResults`
before `search_count` has caught up — the *old* label (`"40 cards match."`)
was staying on screen, presented as authoritative, directly contradicting
the empty state right below it. The reactive graph's version guarantee (the
Open Questions note below) only promises the number itself is never torn or
corrupted — a real answer to a real, now-superseded query — it says nothing
about whether that real-but-stale number is fit to *display unqualified*
next to newer content that has already moved on. That gap is what round 2
(adversarial review, WB-01M0324HQ12B590CZ0YXJPB5T6) closed: `search_count`
now resolves to [`CountPayload`] (`app/src/catalog.rs`) — the count *and*
the query it answers, the same "echo it back" shape `SearchPayload` already
uses for `results` — and the header reuses `Pager`'s own `stale` signal
verbatim (`displaced_by`, comparing the echoed query against the live
URL/box) to dim the line (`opacity-50`, `data-stale`) whenever its own
number has fallen behind, exactly the "inert, not gone" treatment `Pager`
already gives its own links.

Getting the dimming itself to render live cost a second, genuinely
reproduced crash before it worked, and the fix is not what it first looked
like. The obvious-looking gate — read `url_q.get()` synchronously in the
`<Transition>`'s outer closure to skip `search_count` entirely for an empty
query — made that closure a tracked reactive computation, rebuilding a
*fresh* `Suspend` cycle on every settled query change regardless of whether
the *previous* cycle's `search_count` fetch had resolved yet. That premature
rebuild disposed the previous cycle's `displaced_by` signal while its label
was still the content `<Transition>` had on screen; reading it from a live
`class:opacity-50=`/`data-stale=` binding then panicked (`unreachable`, wasm
fatal — took the whole page down, `results` included, not just this line;
reproduced with the held-route idiom, editing a nonzero-hit query to a
zero-hit one). The fix was removing that synchronous read: `search_count` is
now awaited unconditionally, and "was this query empty" is read *after*, off
the resolved `CountPayload`'s own echoed `q` — a fact about the cycle that
already resolved, never a trigger for starting a new one before it has.
Confirmed empirically, not just reasoned: the live binding renders inline at
the call site (no separate component) exactly as safely as it does split out
— the crash tracked to the outer closure's premature rebuild, not to
"inline vs. component." It ships as a separate `#[component]`
(`FilteredCount`, `app/src/catalog.rs`) anyway, as a deliberate tripwire
against a *future* regression reintroducing a synchronous read at that call
site — the same "live signal read inside a child component's own plain view,
not a raw binding built directly as a `Suspend` block's tail value" shape
`Pager` settled on for its own, adjacent round-1 panic (N sibling
`Signal::derive`-holding elements, not this mechanism).

**Anonymous vs signed-in:** `search_catalog_count` (like `search_catalog`)
answers from `HostedBackend::anonymous()`/`NativeBackend::anonymous()` —
catalog-wide, no ownership filter — so the line renders identically for both;
nothing here is session-gated.

## Open questions

- ~~Which Scryfall syntax subset ships in v1?~~ **Proposed above** (the
  table + comma-OR extension) — the acceptance decision.
- ~~Does unrecognized-term preservation need the "N advanced terms" rail
  indicator?~~ **Proposed: no for v1** (see Decisions) — confirm at
  acceptance.
- ~~Server-side: SQL against the ingested catalog, or a Scryfall proxy?~~
  **Resolved by collection-api (accepted 2026-07-14): SQL against our
  catalog.**
- ~~Debounce/latency budget for live results~~ — **closed at 250 ms** by the
  `/catalog` task (2026-07-19), as proposed. The number turned out to be the
  comfort/request-volume knob only: the "no stale results ever render over
  newer input" guarantee is provided by Leptos's `Resource` regardless of the
  delay (reactive_graph's `ArcAsyncDerived` stamps each run with a monotonic
  version and drops a resolved future whose version is no longer latest), so
  tuning the delay cannot break correctness. Caveat recorded there: overtaken
  requests are discarded on arrival, not aborted in flight.
- Whether `colors`/`card_faces` need their own GIN indexes at full catalog
  scale. *(resolved during execution — profile after the stage-2 full load)*
