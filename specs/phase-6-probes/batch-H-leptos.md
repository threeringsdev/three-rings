# Batch H — Leptos internals / error channel

Triage pass, 2026-07-30. Read-only: no file outside this one was modified, no
test/server/DB was touched. Verdicts are against the working tree at
`docs/phase-6-triage` (f9639d6).

Entries: `P6-002`, `P6-003`, `P6-013`, `P6-014`, `P6-022`, `P6-083`.

---

## P6-013 — `initial_value()` ignores `during_hydration()`

**verdict: CONFIRMED** (mechanism), with a **version correction** in the entry text.

**evidence**

- `leptos_server-0.8.6/src/resource.rs:399-405` — `initial_value()` opens with
  `let shared_context = Owner::current_shared_context(); if let Some(sc) = … { let value = sc.read_data(id); … }`.
  No `during_hydration()` guard, no time condition of any kind. Reads for *every*
  `Resource::new`, at any time. Entry's cite (`:399-427`) is accurate.
- `hydration_context-0.3.0/src/hydrate.rs:132` — `read_data` is
  `__RESOLVED_RESOURCES.with(|r| r.get(id.0 as u32).as_string())`: a bare indexed
  read, no take/consume, no clear. Slot survives the read.
- `hydration_context-0.3.0/src/hydrate.rs:143-150` — `during_hydration()` /
  `hydration_complete()` both exist and are maintained over an `AtomicBool`
  initialised `true` at both constructors (`:94`, `:108`).
- `leptos-0.8.14/src/mount.rs:97` **and `:156`** — `sc.hydration_complete()` is
  called from two mount paths, not one. The flag is genuinely maintained; only
  `initial_value` declines to read it.

**version correction (matters for the upstream report).** The entry cites
`hydration_context-0.3.1` and `leptos-0.8.20`. `Cargo.lock` pins
**`leptos 0.8.14`**, **`hydration_context 0.3.0`**, `leptos_server 0.8.6`
(workspace constraint is `leptos = "0.8.2"`). The cited 0.3.1 / 0.8.20 trees are
in the local registry cache but are not what this repo builds. Behaviour is
identical in both — `hydrate.rs:132` (0.3.0) vs `:134` (0.3.1) is the same
expression — so the diagnosis holds; only the coordinates in the report need
fixing.

**new fact: a version bump is not an escape hatch.** `leptos_server-0.8.7` is in
the registry cache and its `initial_value()` (`resource.rs:387-419`) is
byte-identical to 0.8.6's. Nothing has been fixed upstream between the version
this repo pins and the newest one on this machine.

**has anyone filed it?** No evidence. `grep -rn "leptos-rs/leptos" specs/ .github/`
returns **zero** hits; there is no issue/PR number anywhere in the repo, and
`git log -i --grep=upstream` surfaces nothing related. The "is filed" language at
`specs/app-ui.md:330` and `:589` refers to *this backlog entry* ("filed" = written
down here), not to an upstream report — reading it as "already reported upstream"
is the trap, and `P6-002`'s own text ("The durable fix is upstream and **already
filed**") repeats the same ambiguity. As far as this repo can attest, **nothing
has been reported to leptos-rs.**

**is "report it upstream" still the right and only action?** Right, but not
sufficient as written, for three reasons:

1. The action has **no in-repo landing condition**. Filing is S; the *fix*
   shipping is out of this repo's control and cannot gate `P6-002`/`P6-014`
   indefinitely.
2. The report needs the corrected versions above, or a maintainer will bounce it
   against 0.8.20 line numbers that don't match the pinned tree.
3. There is a **second, cheaper local mitigation the entry never considers**:
   nothing in the entry rules out per-resource opt-out at the *call site* (the
   two rejected fixes were both global — clear-on-hydration-complete races
   streaming, clear-on-navigation is too late). Not proposing a fix here; noting
   that "app-side fixes exhausted" is proven only for *general* fixes.

**size: S** (write and file the report). **disposition: KEEP + PROMOTE** — it is
the root cause of the whole serialized-resource defect class and the only entry
in this batch whose resolution retires other entries. Add "correct the version
cites" to its body. **blocked-by:** nothing. **blocks:** `P6-002` (partially).

---

## P6-002 — same-type resource-payload collisions

**verdict: CONFIRMED** structurally; the **latency claim is UNVERIFIABLE** without
running `npm run diag:resource-ids` (`end2end/package.json:23` →
`end2end/measure-resource-ids.mjs`) against a live server, which this pass did not do.

**evidence**

- `app/src/my/all_cards.rs:117` — `struct AllCardsPayload { all_cards: Result<AllCardsView, ServerFnError<String>> }`,
  `#[serde(deny_unknown_fields)]`. Built at `:169` inside `AllCardsBody`'s
  `Resource::new`, and `AllCardsBody` is mounted by **both** `AllCardsPage`
  (`/my`, `:130`) and `AllCardsTablePage` (`/my/all`, `:141`) — one payload type,
  two routes, differing only by `?q=`/cursor. The named field cannot discriminate
  them; the type's own doc comment (`:110-116`) says exactly this.
- `app/src/catalog/destination.rs:339` — `CollectionListPayload { collections: Vec<CollectionSummary> }`
  has two `Resource::new` consumers: the catalog sticky picker
  (`destination.rs:317`) and the tray's move dialog (`app/src/my/move_selection.rs:566`),
  both through `collection_list()` (`destination.rs:345`).
- Quick-add is mounted from `app/src/catalog.rs:888` (`QuickAddButton`); the
  "two collection pages each mount quick-add" pairing was not re-measured.

So the hole is real and there are at least **two** live same-type pairs. What is
*not* re-verified: "measured not to fire at today's id layout" and "~50 slots of
headroom currently protect `/cards/:id`". Both are runtime measurements over a
slot layout that has changed since (the triage doc flags this at line 330). The
exact check is `npm run diag:resource-ids` from `end2end/`.

**size: M** (per-resource request-echo + mismatch-reject + `refetch()`, ×3 pairs
and growing) — **0 if the upstream fix in `P6-013` lands**, which it may never.

**disposition: KEEP** as the surviving id of the pair. Rescope its opening
sentence to drop "already filed" (see `P6-013` — nothing is filed upstream) and
fold in `P6-014`'s one unique asset: the `npm run diag:resource-ids` pointer.
**duplicate-of:** absorbs `P6-014`. **blocked-by:** soft-blocked by `P6-013`;
should **not** be parked on it, since upstream landing is outside this repo's
control.

---

## P6-014 — decode-layer guard can't catch a same-type collision

**verdict: CONFIRMED, and it is the same defect as `P6-002`.**

**evidence** — `app/src/my/all_cards.rs:110-119`: the `AllCardsPayload` doc
comment states `P6-014`'s claim ("two resources of the *same* type can still
cross-decode a correctly-shaped but wrong-query payload (`/my` ↔ `/my/all` with
different `?q=`)") and `P6-002`'s remedy ("Only echoing the request back in the
payload and rejecting a mismatch would close that") in one paragraph. Both
entries cite the same pair, the same measurement, and the same fix; `P6-002`
additionally names `list_collections` and quick-add, so it is the superset.

**size: M** (identical to `P6-002` — it *is* `P6-002`).
**disposition: MERGE→`P6-002`**, carrying over only the
`npm run diag:resource-ids` / `end2end/measure-resource-ids.mjs` sentence, which
`P6-002` lacks. **duplicate-of:** `P6-002`.

---

## P6-003 — `selection_destinations` stays a raw array payload

**verdict: CONFIRMED as a constraint — and the disposition work is already done.**

**evidence**

- `app/src/my/move_selection.rs:546-559` — the raw-array resource is still raw,
  and carries a 14-line in-code comment stating every fact the entry states: the
  `{"Ok":[]}` universal-key hazard, "the tray cannot server-render:
  `SelectionState` starts empty on every load … confirmed by dumping every
  `/my/*` route's slots", and the non-`Copy` → `FnOnce` → view-macro rejection
  cost. `crate::selection_destinations` returns a bare
  `Result<Vec<SuggestedDestination>, _>` (`app/src/lib.rs:1211-1213`).
- `specs/app-ui.md:310-311` already records the declination *with* the non-`Copy`/
  `FnOnce` cost; `:304` records the array-promiscuity fact; `:270-280` records the
  `Option`-only-wrapper-is-decorative fact (bare `null` decodes into every
  `Option`-typed resource); `:490` records the tray-cannot-SSR confirmation.

The triage doc's call ("not a task — a documented constraint, move to `app-ui.md`
Findings") is **right on the classification and stale on the action**: all four
facts are *already* in `app-ui.md`, plus a fuller version in the source itself.
There is nothing left to move.

**size: 0.** **disposition: DROP** the queue entry outright (no migration step
needed). The reopen trigger — "a future change lets the tray SSR" — is already
stated at `app-ui.md:490` beside the evidence, which is the right home for a
conditional. If anything is added, add a one-line pointer from `app-ui.md:490` to
`move_selection.rs:546`; that is bookkeeping, not a task.

---

## P6-022 — `tw_merge` eats the focus-ring colour

**verdict: CONFIRMED, and the entry understates the blast radius by 4 primitives.**

**evidence**

- `app/src/components/ui/item.rs:59` — `ITEM_BASE` carries
  `focus-visible:ring-ring/50 focus-visible:ring-[3px]`, and `:71` passes it
  through `tw_merge::tw_merge!(ITEM_BASE, variant.classes(), size.classes(), class)`.
  Cited line number is still exact.
- **Root mechanism located upstream**, which the entry does not have:
  `tw_merge-0.1.21/src/core/merge/get_collision_id.rs:617-627`. `ring-[3px]` parses
  to elements `["ring"]` + arbitrary `"3px"`; the ring-width arm is
  `["ring"] if arbitrary.parse::<usize>().is_ok()` (`:619`), and `"3px"` does not
  parse as `usize`, so it **falls through to `["ring", ..] => Ok("ring-color")`
  (`:627`)**. `ring-ring/50` lands on the same `"ring-color"` key, last-wins, and
  the colour is dropped. This is a `tw_merge` misclassification of arbitrary ring
  *widths* as ring *colours*, not a class-ordering mistake in `item.rs`.
- **Same pair, same single `tw_merge!` input, in four more primitives**:
  `app/src/components/ui/button.rs:71` (`BUTTON_BASE`),
  `checkbox.rs:27`, `toggle_group.rs:116`, and `input_group.rs:25`
  (`has-[[data-slot=input-group-control]:focus-visible]:ring-ring/50` +
  `…:ring-[3px]` — identical modifier prefix, so identical collision key).
  Every `tw_merge`'d primitive using the arbitrary `ring-[3px]` width loses its
  focus-ring colour, not just `Item`.
- **Not affected**, checked: `input.rs:81-87` and `count_stepper.rs:357` use
  `focus-visible:ring-2`, which hits the `["ring", rest] if rest.parse::<usize>()`
  width arm (`:618`) and does not collide.

Two remedy paths exist (naming them is triage, not prescription): fix it locally
across the five class strings, or report the arbitrary-width misclassification to
`tw_merge` — the second is a sibling of `P6-013` and would fix all five at once.

**size: S→M** (S for the mechanism, M because it is five primitives + a visual
re-verification per primitive, and `Item`/`Button` are the two most-used).

**disposition: RESCOPE** — retitle from "`Item`'s focus ring loses its colour" to
"arbitrary `ring-[3px]` is classified as a ring *colour* by `tw_merge`, so five
primitives lose their focus-ring colour", cite
`get_collision_id.rs:617-627` as the cause and list all five sites. It is an
accessibility-visible defect on the app's two most-used primitives, so it is not
an `Item` footnote. **blocked-by:** none.

**Resolution (2026-08-11):** fixed locally — `ring-[3px]` → `ring-3` in all five
class strings (Tailwind v4 bare numeric width; `"3".parse::<usize>()` succeeds,
so `get_collision_id.rs:618` classifies it as `ring-width` and the collision
disappears). The pinned Tailwind v4.2.1 emits byte-identical
`calc(3px + var(--tw-ring-offset-width))` bodies for both spellings, including
the `has-[…]` compound — exact visual parity. Verified live: a keyboard-focused
`Button` settles at `box-shadow: oklab(0.556 0 0 / 0.5) 0 0 0 3px` (the themed
`ring/50`). Two corrections to the analysis above:

- **`input_group.rs` was never affected.** `tw_merge`'s `parse_variant`
  (`ast/parser.rs:82-92`) cannot consume the `has-[[…]]:` prefix, so the whole
  class fails `parse_style` and is passed through verbatim — it never entered
  the collision set. Four primitives were affected, not five; the change there
  is spelling-consistency only.
- **`ButtonVariant::Destructive` was worse than described**: its own
  `focus-visible:ring-destructive/20` also collided with the misclassified
  width, so destructive buttons had *no* focus ring at all; the fix restores it.

Regression tests pin `BUTTON_BASE` and `ITEM_BASE` surviving `tw_merge` with
both classes intact. Residual polish (doc comments still spelling `ring-[3px]`,
which Tailwind's scanner keeps alive as dead CSS; test coverage for
checkbox/toggle_group; `ring-2` vs `ring-3` width inconsistency with
`input.rs`/`count_stepper.rs`) is filed as a follow-up Workbook task.

---

## P6-083 — server-fn error channel collapses every `ApiError` onto 500

**verdict: CONFIRMED, exactly as written.**

**evidence**

- `app/src/lib.rs:234-236` — `fn api_err(e: shared::ApiError) -> ServerFnError<String> { ServerFnError::ServerError(e.to_string()) }`.
  One arm, no variant match: every `ApiError` becomes `ServerError` → HTTP 500.
  **39 call sites** across `app/src`.
- The JSON channel *does* map correctly: `app/src/backend/routes.rs:672` —
  `StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::BAD_GATEWAY)`,
  over `shared::ApiError::http_status()` (`shared/src/lib.rs:90-99`: 404/401/403/
  409/**422**/502). So the two channels genuinely disagree, which is the entry's
  claim.
- Blast radius today is **status, not text**: `app/src/catalog.rs:134` shows the
  client flattening `ServerFnError::ServerError(msg) => msg.clone()`, i.e. no
  consumer branches on status yet. The damage is that none *can* — a UI that
  wants to distinguish "your input is wrong" (422) from "we broke" (500) has no
  signal, and the native backend's `ApiError::from_wire` (`shared/src/lib.rs:130`)
  round-trip is lossy through this channel.

**size: M** — a custom server-fn error type has to satisfy every adapter and all
39 call sites; `shared::ApiError` already carries the status, so the mapping
itself is trivial and the cost is the type plumbing.

**disposition: KEEP.** Note the relation to `P6-043` (a corrupt catalog cursor
surfaces as `Validation`, and this channel then presents it as a 500) — same
channel, different symptom; `P6-043` gets cheaper once this lands.
**blocked-by:** none.

**Resolution (2026-08-12).** `api_err` now returns
`ServerFnError<shared::ApiError>` (`ServerFnError::from(e)`, a `WrappedServerError`)
instead of flattening to `ServerError(e.to_string())` — same name and
signature, so all ~40 `.map_err(api_err)` call sites (the probe's count of 39
undercounted by one — `move_selection`'s batch-index remap at
`lib.rs:1552` also routes through it) needed no edits. `shared::ApiError`
already carried `Serialize`/`Deserialize`; it needed a new `FromStr` impl
(the left inverse of its existing `Display`) because the server-fn wire
round-trips a custom error's `CustErr` through `Display`/`FromStr` text
(`server_fn::error::ServerFnErrorEncoding`), not serde. The ~30 server-fn
signatures in `lib.rs` (plus the two `collection_backend` helpers) changed
their return type from `ServerFnError<String>` to
`ServerFnError<shared::ApiError>`; every downstream `Resource`/struct-field
type mirroring one of those signatures (catalog, cards, my/collection,
my/needs, my/all_cards, my/tree, my/tree_manage, components/quick_add,
catalog/destination, components/states, bench/states) changed the same way.
`account.rs`/`shell.rs` (the Better-Auth session flow, never `ApiError`-typed)
were untouched.

**HTTP status is still a flat 500 on this channel — confirmed unfixable
without surgery, exactly as sized.** `server_fn` 0.8.8's generic
`Res::error_response` (used by the axum `Response<Body>` impl) hardcodes
`StatusCode::INTERNAL_SERVER_ERROR` for every server-fn error; `FromServerFnError`
has no status hook in this version. The typed variant crossing the wire is
the win regardless — every consumer here derives its affordances from the
`ApiError` variant, not the transport status, so the flat 500 was already
inert to them. `backend/routes.rs`'s JSON channel (still the one with real
status) is unchanged.

**Consumers: typed dispatch first, string fallback second** —
`components/states.rs::describe` (the shared `ErrorNote`/`Failure` classifier
every write surface renders through) now matches `WrappedServerError(ApiError)`
directly via a new `Failure::of_api_error`, falling back to the existing
`classify(&str)` prefix table only for `ServerFnError` variants that never
carried a typed `ApiError` (a dropped fetch, a deserialization failure).
`catalog.rs::describe_error`, `my/tree.rs::tree_retryable`, `cards.rs::classify`
all delegate into it unchanged in shape. Two hand-rolled `validation:`-prefix
constructions (`my/collection.rs`, `my/needs.rs` — a malformed collection id
parsed out of the URL) now build `ServerFnError::from(shared::ApiError::Validation(…))`
instead of a string prefix — the corrupt-cursor-shaped case the probe's
acceptance sketch names, done as typed dispatch rather than a string a
consumer has to parse.

**One real landmine found and fixed, not by inspection but by the e2e
regression run:** `ApiError`'s `#[serde(tag = "code")]` (internally tagged)
had never actually been exercised by `serde_json` before this task —
`to_wire()`/`from_wire()` hand-build/parse `ErrorBody` and never serialize the
enum itself. The moment `ServerFnError<ApiError>` made `leptos_server`'s
resource serialization embed an `ApiError` value directly (for the
SSR→hydration `__RESOLVED_RESOURCES` handoff), it panicked server-side —
`serde_json` cannot merge an internal tag into a newtype variant that
serializes to a bare JSON string (`ApiError::Validation(String)`), so any
`/catalog` request producing a validation error 500'd the whole HTTP
response (`net::ERR_EMPTY_RESPONSE`) instead of rendering the banner. Fixed by
switching to adjacent tagging (`#[serde(tag = "code", content = "message")]`),
which sidesteps the merge requirement entirely; pinned by a new
`serde_json` round-trip test in `shared/src/lib.rs` alongside the
`Display`/`FromStr` one. Caught by `end2end/tests/catalog.spec.ts`'s existing
"a grammar error renders inline" and "a corrupt cursor renders an error with
a way back" tests — both green before this task (the old channel never
serialized `ApiError` directly) and both the exact reproduction once it did.

**Native channel: already lossless, unify not needed.** `backend/native.rs`
already reconstructs a full `ApiError` via `ApiError::from_wire(status, body)`
off the hosted JSON route's real status + body — the native backend was
already as accurate as the wire allows. This task's change means that
already-correct `ApiError` now survives one hop further (through `api_err`
into the server-fn response) instead of being flattened there; no edit to
`native.rs` was needed or made.

**Verification:** `cargo test -p shared` (round-trip both wire shapes — a new
`Display`/`FromStr` test and a new `serde_json` test, one per `ApiError`
variant) and `cargo test -p app --features hosted` (a new typed-vs-string
`describe` test in `components/states.rs`) both green; fmt/clippy (workspace,
wasm, native-backend, both component-bench lines) green; `cargo build -p
three_rings` green. `end2end/tests/catalog.spec.ts` `@fast` (32/32) and
`end2end/tests/states.spec.ts` `@fast` (11/11) green post-fix — including the
two tests that caught the serde landmine above.
