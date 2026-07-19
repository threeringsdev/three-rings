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
