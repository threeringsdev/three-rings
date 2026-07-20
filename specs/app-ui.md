# App UI — the real pages

**Status:** accepted
**Depends on:** [ui-design](ui-design.md), [ui-components](ui-components.md), [ui-component-bench](ui-component-bench.md), [collection-api](collection-api.md), [catalog-search](catalog-search.md), [auth](auth.md), [ui-work-loop](ui-work-loop.md)

## Problem

Every backend spec is `implemented` and every design artifact exists, but the app
still serves the scaffold counter on `/`. The catalog, collections, auth, search,
and tagging surfaces are reachable only via raw JSON routes. This spec defines
the construction of the real UI — the nine wireframed screens, the app shell,
and the three custom gap components — page by page, so each task ships
independently through the merge gate.

Design authority (all Phase 1b deliverables; this spec distills, never
overrides): [`design/information-architecture.md`](../design/information-architecture.md)
(route map, shells, navigation), [`design/wireframes.pen`](../design/wireframes.pen)
(9 screens + overlays + add-flow storyboards),
[`design/add-flow-prototype.md`](../design/add-flow-prototype.md) (keyboard-first
quick-add, time-to-enter-50-cards metric),
[`design/command-palette.md`](../design/command-palette.md) (⌘K),
[`design/component-gap-analysis.md`](../design/component-gap-analysis.md)
(27 primitives: 20 direct, 4 composites, 3 gaps). When a task's acceptance
criteria here feel thin, the wireframes are the source of truth.

## Scope

**In:**
- The full v1 route map (below) replacing the counter, on web and both Tauri shells.
- The app shell: desktop (top-bar mode switch, sidebar rail, docked selection
  tray) and mobile (bottom tabs, drill-down, slide-over filter sheet).
- Component vendoring per the gap analysis (three batches) and the three custom
  gap components: collection tree, in-place count stepper, selection tray.
- Thin per-screen server-fn adapters over the existing `CatalogStore` /
  `CollectionStore` trait methods (the adapters collection-api deliberately
  deferred here).
- Dark palette + migration of existing pages off hardcoded hex onto theme tokens.
- Dev seed data for the test user so `/my/*` screens are buildable.
- The ⌘K command palette (desktop, logged-in).
- E2E coverage per feature (the loop contract lives in
  [ui-work-loop](ui-work-loop.md)).

**Out (parked, per TODO.md Later/parked):** decks-sharing, import/export
(CSV/Moxfield), buy-link integration, format legality, offline bundled catalog,
full-catalog ingest (UI builds against the ~3K-printing POC subset; quick-add
disambiguation realism and list-perf findings are recorded as deferred), app
update delivery.

## Design

### Route map

| Route | Page | Access |
|---|---|---|
| `/` | redirect: authed → `/my`, anon → `/catalog` | public |
| `/catalog` | catalog search/browse | public |
| `/cards/:id` | card detail | public (ownership section authed) |
| `/my` | All cards (My cards landing) | auth |
| `/my/collections/:id` | collection view (binder / deck) | auth |
| `/my/collections/:id/needs` | needs view + pick list | auth |
| `/my/shopping` | shopping list | auth |
| `/login`, `/signup` | auth screens (exist; restyle onto tokens) | public |

Two top-level modes — **Catalog** ("what exists?") and **My cards** ("what do I
have and where?") — switched in the top bar (desktop) / bottom tabs (mobile).
The sidebar rail is mode-filled: filter rail in Catalog, collection tree in My
cards. The selection tray docks at the bottom and survives mode switches.

### Per-page acceptance criteria (distilled; wireframes govern detail)

**`/catalog`** — filter rail (name, card text, set, color, type, rarity, mana
value; multi-selects serialize to comma-OR terms) + query bar above results.
Query text is the canonical state, in the URL (`?q=…&cursor=…`); rail edits
rewrite their term, recognized terms reflect into widgets, unrecognized terms
preserved verbatim (catalog-search contract). Live typing: ~250 ms debounce, one
in-flight request, stale-response discard, first page SSR when the URL carries
`q`. Grid/list toggle; tiles lead with the image (lazy-loaded, skeleton
placeholder). Every result carries `+ Want` / `+ Have` and the sticky
destination picker (`Adding to: 📥 Inbox ▾`, persists across searches).
Logged-out: quick actions prompt sign-in. Mobile: filter rail becomes a
slide-over sheet with an active-filter badge count.

**`/cards/:id`** — full detail: printings, rulings, "your copies & locations"
when authed. Desktop hover on any row/tile opens a lightweight `hover_card`
preview (no URL change); touch tap opens a bottom `sheet` with a "Full
details →" expansion. Multi-face printings must render an image (see the
projection fix below).

**`/my`** — everything-view aggregating all collections incl. Inbox; same row
treatment as collection view but the HERE column is replaced by an expandable
location summary (`7 across 3 collections`). Quick search input, keyset paging.

**`/my/collections/:id`** — child collections as folder rows on top, cards
below. Three right-aligned numeric columns under one header: HERE / WANTED /
OWNED (WANTED only when set and different; OWNED collapses when equal to HERE;
rolled-up child counts italic + dimmed). HERE is editable in place via the count
stepper. Persistent in-collection quick-search/type-ahead in the header (`/`
focus hint) that filters this collection and inline-adds catalog matches — the
intake path. Per-row move (kebab / swipe / `m`) and select (checkbox /
long-press / `x`) affordances. **Deck variant** adds: format + commander(s)
rendered as a card in the header, cards grouped by type with counts, the needs
chip (`6 missing — 4 owned elsewhere · 2 to buy`), Want-led add default (binders
and Inbox are Have-led), and the "Empty deck…" teardown action (single
destination or "Return to previous locations").

**`/my/collections/:id/needs`** — two buckets: **Owned elsewhere** rows show
where copies live + a one-tap **Pull** (pre-filled move); **Pull all** generates
a pick list (checklist grouped by source collection; checking records the
move). **Short** rows feed the shopping list. Pull/pull-all are client-composed
from `move_cards` + `suggested_destinations` (collection-api Findings).

**`/my/shopping`** — one row per card: shortfall count + which collections want
it; text export.

**App shell** — desktop: slim top bar (mode switch `Catalog | My cards`, user
menu/avatar), sidebar rail, main panel, selection tray docked bottom, undo toast
on every move. Mobile: two tabs `[📖 Catalog] [🗂 My cards •N]` (badge = Inbox
unsorted count); My cards is drill-down (root mirrors the sidebar; back walks
up); tray docks above the tab bar and survives tab switches.

**⌘K palette** — desktop, logged-in only. Places (flattened collections with
parent-path meta + system places + mode jumps) and a fixed 3-command registry
(`New binder…`, `New deck…`, `Undo last move`); at rest RECENT + COMMANDS,
first row pre-selected. Client-side filter over a preloaded index.

**Quick-add panel** — the central intake composite (`command` + `popover` +
`input` + `kbd`). Keystroke contract from the storyboards:
`↑↓ navigate · ⏎ add 1 here · ⇧⏎ set count · ⌥⏎ want instead`; desktop steady
state ≈ 5–7 keystrokes/card, zero pointer. Deck context flips the default to
Want. E2E asserts the keystroke contract.

### Custom gap components (bench section required, like any vendored component)

1. **Collection tree** — the largest: nesting, per-node collapse, pinned system
   rows (All cards / Inbox / Shopping list), selection, rolled-up count badges,
   drag reparent/reorder, context-menu management. Built in two tasks
   (read-only, then management) on `collapsible`/`item`/`badge`/`context_menu`.
2. **Count stepper** — hover/focus-revealed `− n +`, click-to-type, keyboard ±
   on the focused row, commit-on-blur; optimistic update + undo toast. Composed
   from `button` + `input`.
3. **Selection tray** — docked thumbnail stack + count + "Move to…" + clear;
   cross-view selection state. (Registry `action_bar` was evaluated and ruled
   out in the gap analysis.)

### Conventions established by this spec

**Thin server-fn adapters.** No per-op Leptos server fns exist (collection-api
deferred them here). Each page task adds only the adapters it needs, as thin
projections of trait methods — no business logic. Exemplar shape:

```rust
#[server]
pub async fn search_catalog(q: String, cursor: Option<String>) -> Result<SearchResults, ServerFnError> {
    let headers: http::HeaderMap = extract().await?;
    let backend = server_backend(&headers); // anonymous or session-scoped
    backend.search(SearchQuery { q: Some(q) }, Page { cursor, limit: None })
        .await
        .map_err(api_error_to_server_fn)
}
```

**`/my/*` auth guard.** No client-side guard exists today. Pattern: a shared
wrapper component holding a `Resource` on `fetch_current_user`; anon →
`use_navigate` to `/login?next=<current>`; `/login` honors `next` after
sign-in. Server fns underneath still enforce auth independently
(`user_id_from_headers`) — the guard is UX, not security.

**Tokens, not hex.** All new UI uses the theme-token utilities
(`bg-background`, `text-muted-foreground`, …). A `.dark` block is added to
`style/input.css` (OKLCH values for every token) and the existing hardcoded-hex
pages (`HomePage` remnants, `auth_pages.rs`) migrate. Theme class rides `<html>`
with persistence (model per Open question below). The counter and its
`get_count`/`increment_count` server fns + `storage` module are deleted with the
shell task.

**Vendoring.** Via the `vendor-component` skill: bench section in the same
commit, runtime verification checklist including native webviews. The six
interactive components (dialog/popover/command/hover_card/sheet/sonner) are
"vendor markup + CSS, rewire behavior in Leptos" — deterministic caller-supplied
IDs (no `use_random_id`), Leptos-owned open state, verified CSS-anchor
positioning fallback for WKWebView / Android WebView. `command` is the shared
core of quick-add, destination picker, and ⌘K — its reactive rewiring happens
once, in the vendoring batch, not per-feature.

### Known defect folded in

**Multi-face card images**: `HostedBackend` projections read
`image_uris->>'normal'`, which is NULL for `transform`-layout printings
(Scryfall nests `image_uris` per face) — blank tiles today. Fix in the
card-detail task: `COALESCE(p.image_uris->>'normal', p.faces->0->'image_uris'->>'normal')`
in the summary/detail projections. Images hotlink Scryfall's CDN (policy-fine
at this scale); no image pipeline this phase.

## Open questions

None — all resolved at spec review (maintainer, 2026-07-17):

- **Theme persistence** — **dark mode is the default**; an explicit toggle
  override is persisted as a saved user preference. The dark-palette task wires
  accordingly (default `dark` class on `<html>`, override stored and re-applied
  SSR-safely).
- **Sonner engine** — **small native Leptos toaster**, not upstream's vendored
  JS engine (undo-on-toast wants first-class Leptos state; upstream's own
  `_sonner_leptos_only_later/` points the same way). Accepted deviation from the
  vendor-as-is convention.
- **POC catalog** — **deferred, confirmed**. Quick-add disambiguation realism
  and list performance are explicitly not goals of this phase; the phase's goal
  is validating the infrastructure already built (API, ingestion, design system,
  auth, search). Data-scale issues are addressed after the full ingest (parked
  task + Later/parked note in TODO.md).

## Findings

(appended per task by the work loop — decisions, surprises, disputed review
findings with rationale, deferred items)

### Catalog page `/catalog` (2026-07-19)

`app/src/catalog.rs` — query bar, results grid/list, view switch, anonymous
quick actions; `search_catalog` server fn in `app/src/lib.rs`. The rail,
destination picker, and mobile filter sheet are their own queued tasks.

- **`search_catalog` is the adapter exemplar, and it is a `GET`**
  (`input = leptos::server_fn::codec::GetUrl`) rather than the server-fn POST
  default. Two reasons, both load-bearing: it's a pure read whose arguments
  belong in a cacheable URL, and the Tauri Android dev proxy strips POST
  bodies (ui-work-loop Findings), which would have made the on-device search
  unverifiable. **Verified on-device**: typed search returns results through
  the proxy. Later read adapters should copy this; write adapters can't.
- **Opportunistic auth reuses `routes::catalog_backend`** (promoted to
  `pub(crate)`) instead of re-deriving the rule. Two callers disagreeing about
  when a catalog read is session-scoped is exactly the drift the seam exists
  to stop. The `native` arm expresses the same rule via
  `NativeBackend::authed` with possibly-absent session material.
- **Stale-discard is the reactive layer's, not ours** — claim verified against
  the source, not assumed: `Resource` is an `ArcAsyncDerived`, which stamps
  each run with a monotonic version and drops a resolved future whose version
  is no longer latest (reactive_graph 0.2.14 `arc_async_derived.rs`,
  `if latest_version == this_version`). So catalog-search's "no stale results
  ever render over newer input" holds independent of the debounce. **Debounce
  closed at 250 ms as proposed** (catalog-search open question). What we do
  *not* have is cancellation: an overtaken request is discarded on arrival,
  never aborted — the debounce is what limits request volume.
- **History is per search session, not per keystroke.** Refining replaces,
  starting/ending a search pushes. Replace-always (the obvious reading of
  "one entry per session") was implemented first and caught in probing: Back
  from the first typed search walked straight off the site.
- **View mode rides `?view=list`.** It is not search state and never enters
  the query text, but keeping it in the URL makes it SSR correctly (no
  post-hydration flip) and survive reload/share.
- **Grid/list is a real `radiogroup` with roving focus** — the behavior V1's
  vendoring parked here. `toggle_group`'s hardcoded `tabindex="-1"` became a
  prop (deviation noted in-file); arrow-key selection + focus movement stays
  feature-side, with catalog.rs as the reference wiring and a new bench
  assertion pinning "exactly one tab stop, on the selected item".
- **Two Leptos traps, both cost a debug cycle.** (1) `{..}` spread after a
  path-valued prop (`bind_value=query_text {..}`) parses as *struct-update
  syntax* and the spread silently vanishes into the value — put a
  paren/string-terminated prop last. (2) Reading a signal in a `#[component]`
  **body** bakes the value in at construction; the layout switch stopped
  working until the `list_view` read moved into a closure. The regression
  test written minutes earlier is what caught it.
- **Result counts are `N` / `N+`, not a total.** The endpoint is keyset-paged
  and deliberately runs no `COUNT` (catalog-search), so the wireframe's
  "128 results" is not obtainable; "at least N" is the honest rendering.
  Paging beyond the first page (`?cursor=`) is **deferred** — filed as a
  follow-up task rather than absorbed.
- **Codex adversarial review — 2 fixed, 1 accepted-as-documented, 1 disputed:**
  - *Grammar errors blanked the result set* (high) — **confirmed by probe**
    (grid count 1 → 0 on `bolt pow>3`) and **fixed**: the last OK page is
    retained and rendered dimmed/inert under the error, since half-typed
    queries hit the term-naming 422 constantly. Regression test added.
  - *URL⇄field sync could clobber newer typing* (high) — **not reproducible**
    (the window is between `navigate()` and the effect flush, sub-millisecond;
    timed-keystroke probing at 240 ms spacing never hit it) but the mechanism
    is real: the effect had no way to tell our own navigation from an external
    one. **Fixed** by tracking the last self-pushed query and re-seeding the
    field only on external URL movement.
  - *No cancellation of in-flight requests* (medium) — **accepted, not fixed.**
    The "no stale render" guarantee holds via the version counter (Codex
    verified this rather than refuting it); true abort isn't exposed through
    the server-fn client. The overstated "one in-flight request" wording in
    our own doc comment was corrected instead.
  - *`ApiError::Validation` reaches the UI as HTTP 500, not 422* (medium) —
    **disputed.** This is the pre-existing, documented behavior of the shared
    `api_err` helper (lib.rs: "the transport channel carries the message;
    richer status semantics are collection-api's"). The status-correct channel
    is the JSON API (`ApiError::http_status`), which is what the native backend
    consumes; the Leptos server-fn channel is UI-internal and reads the
    message. Making it status-accurate means a custom error type across every
    server fn — filed as a follow-up task, not smuggled into this one.
- **E2E mutation pass — 11/11 kills**, and it found two tests that passed
  vacuously before it: the lazy-image assertion was wrapped in
  `if (await img.count())` (a page rendering no images at all would have
  skipped it), and "a signed-in visitor gets no sign-in prompts" would have
  passed if the quick actions were deleted outright. Both strengthened, plus
  the retained-results test now asserts the *actual cards* survive and a new
  test pins `&`/`+` (the characters a naive encoder splits or eats) through a
  full type→URL→reload round trip. Mutations confirmed killed: debounce
  250 ms→31 s, `replace` forced false, `last_good` never retained, lazy→eager,
  `authed` forced false, validation prefix renamed, encoder ignoring its
  input, focus index pinned to 0, view switch dropping `q`, clear not
  committing, and `url_q` ignoring `?q` — the last also kills the SSR test,
  which is what proves that test asserts *`q` drove the search* rather than
  merely that SSR happened.
- **Verification**: web SSR asserted at request level (rendered results in raw
  HTML for `?q=`), hydration CLEAN on 4 URLs (browse-all, search, list view,
  error), bench-check CLEAN, fast tier 25/25, Android debug webview 8/8
  on-device (incl. touch view-switch and zero horizontal overflow at phone
  width).
- **Known-cosmetic**: transform-layout printings still render the skeleton
  instead of an image (`image_uri` NULL until the card-detail task's
  `COALESCE` fix) — the tile degrades to skeleton + name rather than breaking.

### App shell + routing (2026-07-19)

The shell (`app/src/shell.rs`): top bar (brand, desktop mode switch, theme
toggle, user menu popover), sidebar rail frame, mobile bottom tabs, route
skeletons for the full map, `/` redirect, `/my/*` guard. Counter +
`get_count`/`increment_count`/`storage` deleted; `/cards` placeholder folded
into `/catalog` (keeps the seam-proving `catalog_count` read).

- **Server-side redirects are real 302s.** `/` and the `/my/*` leaves run
  `SsrMode::Async` so `leptos_axum::redirect` can still set status before
  streaming; `<Redirect/>` covers SPA navigation. Gotcha: leptos_axum only
  emits 302 when the request's `Accept` contains `text/html` — curl's default
  `*/*` gets 200 + `Location` + `serverfnredirect` headers instead. Probe with
  `-H "Accept: text/html"`.
- **Webview redirect-swallowing shim.** The Tauri Android webview fetches
  documents through an in-process proxy that follows 302s internally, so the
  webview receives the redirect *target's* HTML at the *original* URL —
  hydrating panics (router renders the URL's route against the target's DOM;
  reproduced on-device at `/` and `/my`). `shell()` stamps
  `data-ssr-path` on `<html>` (outside the hydrated root, like the theme
  class); `shell::hydrate_entry` compares pathnames before hydrating and
  `location.replace`s to the stamp on mismatch. Real browsers never mismatch.
- **One shared current-user resource** (`CurrentUserResource` in context) for
  redirect + guard + user menu. Consequence: auth transitions must be
  **full-page loads** (`redirect_browser`/`hard_navigate`), not SPA
  navigation — the login fixture caught sign-in dispatching on the stale
  anonymous resource and landing on /catalog. All five success paths
  (password, signup, reset, OTP, Google-Tauri poll) and sign-out do document
  loads now.
- **Guard reads location untracked.** A tracked read re-ran the guard's
  Suspend mid-redirect and compounded `next=/login%3Fnext%3D…` (found
  on-device, applies to web SPA nav too). E2E now pins the single bounce.
- **Codex review** (commit 4fb0241): 1 high — `next=/\evil.com` open
  redirect (browsers normalize `\`→`/`) — fixed by also rejecting `/\`;
  1 medium — Google sign-in loses `?next` (web OAuth callback 303s to `/`,
  Tauri poll hard-loads `/`) — deferred as a queue task, sign-in still lands
  correctly via the `/` redirect, just not on the guarded page; 6 explicit
  clean confirmations (route map, mode-switch/tabs breakpoints, counter
  deletion, stale-resource paths, hydration-safety conventions).
- **Codex e2e mutation pass**: 5 assertion strengthenings applied (exact
  `<h1>Catalog</h1>` SSR regex, post-settle `?next` stability re-assert on
  the SPA-bounce test, `aria-current` asserts on the mode switch, exact
  email in the user-menu assert, cookie-presence check on the saved
  storageState). All 10 tests then demonstrated kills in three transient
  mutation rounds: A (catalog h1, guard next-path) killed the SSR, guard
  and SPA-bounce tests; B (root-redirect target, mode-switch link,
  login-redirect hardcode, bottom-tab link) killed the 302, mode-switch,
  next-honoring and tabs tests; C (authed target, user-menu text, post-auth
  default) fails the login fixture itself, blocking the suite — no
  decorative tests. All mutations reverted; suite green after.
- **Android dev-proxy strips POST bodies and Cookie headers** (verified
  directly: argless server-fn POST → 200, form POST arrives with empty body →
  "missing field email", valid injected session cookies → `/api/me` 401).
  Authed flows are unverifiable over the dev-attach webview; on-device
  verification covered the anon surface (redirect, tabs, guard bounce, shim
  recovery — 5/5 PASS) and the authed surface is covered by the web tiers.
  Release-path auth is unproven → queue task before the phase-end smoke.
  Details in ui-work-loop Findings.

### Vendor batch V3 — command / hover_card / sonner (2026-07-19)

The interactive core of the app's central surfaces. All three carry
data-name markers and their bench sections in the same commit.

- **`command` fully reactive** (the gap analysis's headline rewrite): the
  parallel vanilla-JS keyboard+filter script — which fought the reactive path
  by *also* writing item visibility — is deleted. Filter is a per-item
  `Memo`; ↑↓/⏎ navigation is a Leptos **item registry** (each `CommandItem`
  registers on mount into a shared `RwSignal<Vec<ItemReg>>`, deregisters on
  cleanup), and `CommandInput` drives a `highlight` index over the *visible*
  subset. `CommandEmpty` reacts to "no item visible". This is the shape
  features extend with ⇧⏎/⌥⏎/count-entry by reading modifiers in their own
  handlers. `CommandDialog` wraps it in the vendored `dialog` (deterministic
  caller id, Leptos open state) — no inline script.
- **`hover_card`**: native Popover API + CSS anchor positioning kept;
  hover-intent is a cancelable Leptos `TimeoutHandle` (150 ms open on
  enter/focus, 150 ms close on leave/blur, cancel-close while over the
  content) — upstream's inline `<script>` gone. No JS position fallback (a
  hover preview is never the sole affordance; cosmetic if mispositioned).
- **`sonner` is a native Leptos toaster, not a vendored copy** (maintainer
  decision, Open questions): upstream's Rust side is markup that triggers a
  separate `sonner.js` engine we don't ship. Ours: a `Toaster` mounted once
  provides a `ToastHandle` via context; `handle.show(ToastOptions…)` fires
  programmatically with an optional **action button** (the undo-on-toast the
  move flow needs) and auto-dismiss. API shape follows the registry so
  callers read familiarly.
- Bench-check extended: command reactive filter + ↑↓/⏎ selection, toast fire
  + undo-dismiss, hover-intent open/close. Snag caught in-loop: the bench
  first rendered `CommandInput` without a `<Command>` ancestor →
  `expect_context` panicked in SSR and killed the server thread; wrapping in
  `<Command>` fixed it (a good reminder the context is mandatory).
- **Codex review** (9 findings) — sonner cleared entirely (native design
  accepted; auto-dismiss/keys/id-allocation all verified correct):
  - **hover_card trigger→content handoff broken** (#3, high) — trigger and
    content held *separate* `HoverTimer`s, so moving onto the card didn't
    cancel the trigger's pending close and it shut ~150 ms later. **Fixed**:
    one timer shared through the context; both endpoints cancel/reschedule
    the same handle. The bench now moves onto the content and asserts it
    stays open (was untested — finding #6), plus `on_cleanup` cancels a
    pending timer on unmount (#4).
  - **command highlight not clamped on shrink** (#2, medium) — a stale
    highlight above a shrunk visible set rendered no selection. **Fixed**:
    the highlight memo and Enter both clamp to the last visible row.
  - **command registry vs DOM order after in-place keyed reorder** (#1) —
    **documented as a bounded limitation** (module doc + here): all three
    consumers are append-only (static client-filtered lists; full remounts
    on new server results), none reorder persistent items in place, so the
    `compareDocumentPosition` sort is deferred until one does. Not a live
    bug for any current consumer.
  - Remaining bench-depth findings (#5, #7–#9) acknowledged; the `.mjs`
    probe stays diagnostic-grade (behavioral depth is the per-feature e2e's
    job) — the two that pointed at real defects (#6 handoff) are now
    covered.

### Vendor batch V2 — overlay foundations (2026-07-19)

`scroll_lock` + `dialog` + `popover` + `sheet`, markup/CSS vendored from
rust-ui@43e1e32, behavior rewired to Leptos (the gap analysis's
"vendor markup + CSS, rewire behavior" plan):

- **scroll_lock is the pure-Rust registry hook** (not the JS asset):
  hydrate-gated implementation + no-op SSR stubs, and the `window.ScrollLock`
  JS-interop registration dropped — no inline scripts remain to need the
  global. Let-chains rewritten for the 2021 edition.
- **Deterministic caller-supplied `id`s everywhere** (the `use_random_id`
  SSR-counter hydration bug from the gap analysis is structurally gone).
- **One `RwSignal<bool>` owns each overlay**: trigger/close/backdrop/ESC all
  drive it; callers can pass their own signal for programmatic open (the
  `m`-key move flow) — proven in the bench via a programmatic-open button.
  ESC listeners are `window_event_listener` handles removed on cleanup
  (upstream leaked per-instance `document` listeners). Closed panels get
  `inert` (upstream's closed overlays stayed keyboard-focusable —
  `pointer-events-none` only blocks the mouse).
- **Popover keeps the native Popover API + CSS anchor positioning** and
  gains two-way sync: signal→`showPopover`/`hidePopover` in an Effect,
  native `toggle` events→signal via `:popover-open` (DOM types through
  leptos's own web_sys re-export — compiles in every build). The
  close-on-CommandItem inline script is gone; compositions use
  `use_popover_open`.
- Sheet's open/closed transform is a reactive class (upstream mutated
  classList from its script); direction enum hand-written (no strum).
- **Anchor positioning verified on the Android webview on-device** (Chrome
  145, `CSS.supports` true, panel 9 px off the trigger). A **JS positioning
  fallback** lands anyway (spec requirement): `web_sys::css::supports` gates
  it; when anchors are absent the panel is fixed-positioned under the trigger
  (flipping above on viewport overflow). The installed WebKit build also
  reports support, so the fallback is defensive — exercised by construction,
  not observed firing.

**Codex review (9 findings + 1 extra) — a genuinely valuable pass, 8 fixed:**
- **Stacked-overlay ESC closing everything** (#1) + **ESC via
  stop_immediate_propagation**: new `overlay_stack` module (client-only,
  ssr-stub); each overlay pushes its id on open, and the ESC handler acts
  only when it's the topmost — *and* calls `stop_immediate_propagation` so a
  synchronous signal-flush of the stack can't let the next overlay's window
  listener fire on the same keypress. Proven: sheet+dialog open, one ESC
  closes only the dialog, a second closes the sheet.
- **Scroll lock not reference-counted** (#2) + **unlock-delay reopen race**
  (#3): `scroll_lock` gained an owner count + a generation counter. Stacked
  overlays share one lock (last-out unlocks); a delayed restore no-ops if the
  generation moved (close-then-reopen keeps the DOM locked) or an owner
  remains. Bench asserts body `overflow:hidden` engages on open, survives
  while a second overlay is open, and releases only when the last closes.
- **Popover JS fallback absent** (#4): added (above); `show_popover` failure
  now re-syncs the signal from the DOM instead of silently drifting.
- **Closed overlays keyboard-focusable** (folded into #8/aria): `inert` on
  closed dialog/sheet panels; `aria-label` prop added (Titles alone gave the
  overlays no accessible name — the extra finding).
- Bench (#5–#7, #9) strengthened: horizontal popover overlap check, the
  stacked-ESC scenario, scroll-lock body-style assertions, and a two-render
  ID-stability diff. **Disputed**: #8's "children always instantiated" is
  inherent to this always-mounted overlay pattern (upstream's too); revisited
  only if a specific overlay's content proves expensive — noted, not changed.

### Vendor batch V1 — static primitives (2026-07-19)

Eleven components vendored from rust-ui@43e1e32 (button, badge, input,
input_group, kbd, separator, checkbox, label, toggle_group, breadcrumb,
skeleton, card), each with a bench section in the same commit. Batch-wide
decisions (per-file details in each header):

- **`variants!` hand-expanded.** Upstream's 457-line `leptos_ui::variants!`
  macro (plus its `paste` dep and `TwClass`/`TwVariant` derives) is replaced
  with plain enums + `match` arms carrying identical class strings — zero new
  dependencies, and the token trap surfaces at review time instead of
  silently emitting no CSS.
- **Undefined-token variants dropped**: button `Warning`/`Success`/
  `Bordered`, badge `Success`/`Warning`/`Info` (they reference `warning`/
  `success`/`info`/`*-light`/`*-dark` tokens style/input.css doesn't define).
  Re-add variants together with their tokens if a screen needs them.
- **`void!` joined `clx!`** in the vendored clx.rs (same leptos_ui source).
- **Icons inlined** (checkbox check, breadcrumb chevron/ellipsis — Lucide
  paths, ISC) rather than adopting the registry's icons crate.
- **Upstream bug fixed as deviation**: label's runtime-formatted named-peer
  classes (`peer-disabled/{for}:…`) can never have CSS generated for them —
  replaced with the static `peer-disabled:` pair.
- **`strum` avoided** (input's type enum → hand-written `as_str`);
  **`InputGroupTextarea` dropped** (no textarea vendored, no wireframe use).
- Component attr pass-through convention: `attr:aria-label=…` etc. on the
  component tag (the `{..}` spread form mis-parses hyphenated attrs in this
  leptos version).
- Verified: bench-check extended (SSR marker per family + checkbox/
  toggle-group interaction + the html-level bench toggle) — CLEAN; fast tier
  4/4; **Android webview on-device** (all families render, checkbox
  interacts). ID stability N/A (no generated IDs); assets N/A (none
  referenced).
- **Codex review** (9 findings): 1–4 ("`attr:` on components can't compile /
  won't forward") **disputed with hard evidence** — both clippy halves green
  and all five spot-checked attributes (`href`, `data-slot`, `aria-label`,
  `role`, `aria-current`) present in the served SSR HTML; `attr:` on a
  component is Leptos 0.8's documented root-attribute pass-through, and
  upstream's own breadcrumb uses it on clx components. 5 **accepted**:
  `aria-checked` added to ToggleGroupItem (deviation noted in-file); roving
  focus/keyboard is feature-side, lands with the catalog switch. 6–7
  **accepted**: bench demo now exercises the label↔checkbox `for`/`id`
  association and the probe asserts it plus the toggle item's `data-state`.
  8–9 (probe depth) **disputed**: the `.mjs` probe layer is cheap
  diagnostics by design; behavioral depth belongs to the per-task e2e specs
  (ui-work-loop's tier contract).

### Dark palette + token migration (2026-07-19)

- **Token set**: `style/input.css` now carries the full Rust/UI standard set
  (background/foreground, card, popover, primary, secondary, muted, accent,
  destructive + foregrounds, border, input, ring) in `:root` *and* `.dark`
  (upstream OKLCH values, charts/sidenav trimmed), plus
  `@custom-variant dark` and a base `body { bg-background text-foreground }`.
  Full set added now so Stage 1 components land without token churn.
- **Dark is the default**: `shell()` reads the `tr_theme` cookie from the
  request `Parts` in context and stamps `class="dark"` (absence of cookie or
  any non-`light` value = dark) on `<html>` during SSR — right before any
  wasm runs, no flash. `<html>` attrs live outside the hydrated root, so the
  client toggle owns them post-hydration; no mismatch by construction.
- **theme_toggle vendored** (deviations in its header): upstream's `icons`
  crate inlined as two SVG paths; `use_theme_mode` hook replaced with
  app-owned state — toggle flips the class and persists
  `tr_theme=light|dark` (1 year, SameSite=Lax). Bench section is live
  against the real page theme (unlike the bench-local toggle).
- **Hex migration**: HomePage + auth_pages fully on tokens (auth_pages was
  conveniently constant-driven). The scaffold teal CTA became `bg-primary`
  (the wireframes are grayscale; a brand accent is a later design decision).
  Deliberately NOT migrated: the two standalone bounce/callback HTML strings
  in lib.rs (raw documents served without the stylesheet — tokens can't
  reach them).
- Verified live: 6/6 theme-probe checks (dark default SSR, toggle flips
  class+cookie, both overrides survive reload SSR-side, raw no-JS SSR honors
  the cookie), hydration clean on 4 routes, fast tier 4/4, **and the Android
  webview** (dark default + toggle flip on-device over CDP — matrix path 1).
- **Codex review** (3 findings, all accepted + fixed): production had no
  toggle mount until the shell lands → interim footer mount on HomePage (two
  lines the shell task deletes); light-override icon flash → the signal now
  initializes from the cookie on BOTH sides (`cookie_theme_is_dark()`: Parts
  SSR-side, `document.cookie` client-side — the cookie is deliberately not
  httpOnly), removing the corrective Effect entirely; the bench-local toggle
  couldn't show light under the dark `<html>` default (container-scoped
  class can't override ancestor variables) → the bench control now drives
  the `<html>` class directly, session-only, no cookie. All re-verified
  (6/6 probe, hydration clean, fast tier 4/4).

### Dev seed data (2026-07-19)

`app/src/seed.rs` (hosted-only) + the `server --seed-dev <uuid>` CLI arm
(mirroring `--ingest`) + `scripts/seed-dev-data.sh` (resolves the e2e user's
uuid owner-side, then runs the seed as `app_runtime`). Decisions:

- **Real methods only** — every write goes through `CollectionStore` /
  `CatalogStore` (search → card_detail → first printing), so the seed
  exercises the same paths the `/my/*` screens read back, including the lazy
  Inbox provision, RLS under `scoped_tx`, and intake `moves` rows.
- **Shape**: Inbox (4 arrivals) · Trade Binder (6 cards, one foil playset) ·
  Shoebox ▸ Rares (nested) · "Commander Deck" (format=commander; commander
  system-tagged; 7 mainboard + 1 sideboard; 2 wants held in Trade Binder →
  the owned-elsewhere needs bucket, 2 wants held nowhere → short/shopping) ·
  1 explicit move (Trade→Shoebox) for undo/pull history.
- **Idempotency = sentinel** ("Trade Binder" exists → no-op). Chosen over
  delete-and-rebuild: re-seeding from scratch is `end2end/seed-e2e-user.sh`
  with a fresh `.env` (recreates the user; collections cascade). Verified:
  first run wrote {4 collections, 20 holdings, 4 desires, 1 move}; re-run
  no-oped; dev-branch SQL shows 5 collections / 29 copies / 4 desires /
  21 moves / 1 commander tag.
- Seed queries fail loudly (`found x/n — is the POC catalog ingested?`)
  rather than building a partial tree.
- **Codex review** (7 findings): partial-tree-behind-sentinel + non-atomicity
  → **fixed** with cleanup-on-error (created roots deleted best-effort; a
  wrapping tx is impossible through the store methods, deliberately);
  `--seed-dev` shipping in the release binary (unlike `--ingest`, no
  dedicated credential) → **fixed** with `#[cfg(debug_assertions)]` — release
  binaries don't carry the arm at all; owner-credential SQL interpolation +
  PG env inheritance in the scripts → **fixed** (psql `:'email'` variable via
  stdin — note `-c` never interpolates variables — and per-invocation PG*
  env), same hardening retrofitted to `seed-e2e-user.sh`; name-based
  sentinel spoofable → **disputed**: the e2e account is purpose-built and
  script-owned by contract. All fixes re-verified live (idempotent no-op
  path + fresh-user path).
