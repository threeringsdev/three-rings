# P6-017d follow-up — does the delete confirm name the collection, and does the dialog survive the transition?
Probed 2026-07-29 against f9639d6.

## Answer
**The user is not misled.** The confirm's title is `format!("Delete {name}?")` and the `name`
comes from the *same* `DeleteReq` snapshot that `submit_delete` deletes — so in the transition
window the dialog says "Delete A?" and deletes A. Nothing in the dialog reads the URL, the tree,
or the live `view_res`. Q2 reinforces this: the dialog is mounted at the **app shell**
(`shell.rs:292`), a sibling of `<main><Outlet/></main>`, entirely *above* both `<Transition>`
blocks in `collection.rs`, and its state is shell-owned. When B's payload lands, the header
underneath swaps to B while the open dialog keeps showing A's name and A's counts and still
deletes A. The residual defect is therefore **not** a mis-aimed or mislabeled delete; it is the
narrower and much milder one that the *page behind* the dialog can change identity while the
dialog is open, so the confirm names a collection the visible header no longer shows. The
dialog is the half telling the truth. Whether the user is misled thus depends entirely on
whether they read the dialog they are answering, and the dialog names its subject in its title.

## Q1 — what the confirm displays
- **Dialog title** → `` format!("Delete {name}?") `` — `app/src/my/tree_manage.rs:733`
  (inside `<DialogTitle>` at `:730-735`; fallback `"Delete?"` at `:734` only when
  `delete_req` is `None`, which cannot be the case while the dialog is open). **Source: the
  snapshot.** It reads `delete_subject()` (`tree_manage.rs:604-609`), which is
  `manage.delete_req.get().map(|r| (r.name.clone(), r.descendants(), r.cards))`.
- **Dialog description** → `` format!("This permanently deletes {what} inside it. This cannot be undone.") `` —
  `app/src/my/tree_manage.rs:752-754`, where `{what}` is assembled at `:741-751` as
  `"{descendants} nested collection{s} and "` (only when `descendants > 0`) + `"{cards} card{s}"`.
  **No name appears in the description** — it says "it", deferring to the title. Counts come
  from the same snapshot tuple.
- **`DialogContent` accessible name** → `aria_label="Delete collection"` — `tree_manage.rs:727`.
  Generic; no name.
- **Button labels** → `"Cancel"` (`DialogClose`, `tree_manage.rs:762`) and `"Delete"`
  (destructive `Button`, `tree_manage.rs:769`). Neither carries a name.
- **Nothing else in the dialog renders text.** Only `error_line` (`:760`, server message on
  failure) is left, and it carries no name.

### The snapshot chain, end to end (all one collection, A)
1. `collection.rs:651` — `let collection = StoredValue::new(view.collection.clone());`, a
   snapshot of the payload that rendered this header.
2. `collection.rs:659-666` — the `aim` callback sets `manage.menu_target` from
   `MenuTarget::for_collection(&collection.get_value(), &roots, cards_here + here_delta)`.
3. `tree_manage.rs:320-344` — `open_delete()` reads `menu_target.get_untracked()` and copies it
   into a `DeleteReq { id, name, subtree, parent_id, cards }`.
4. `tree_manage.rs:604-609 → :733 / :752` — the title and description render from that
   `DeleteReq`.
5. `tree_manage.rs:556 / :568` — `submit_delete` reads `manage.delete_req.get_untracked()` and
   calls `delete_collection(req.id)`.

Steps 4 and 5 read **the same struct**, so the name displayed and the id deleted cannot diverge.

- Worth recording because it is the one count that *could* have drifted: the card count in the
  description is `cards_here + here_delta` captured at aim time (`collection.rs:664`), and
  `here_delta` is deliberately zeroed **by a landing payload, not by the URL**
  (`collection.rs:206-221`, `if view_res.get().is_some()`). During the in-flight window
  `here_delta` therefore still belongs to A's payload — the exact case that comment calls out
  ("a navigation zeroed it while the `Transition` was still showing the pre-commit totals it
  belonged to"). The count is A's too.

## Q2 — the dialog across the transition
- **The dialog is above the transitioned subtree, not inside it.** `<TreeDialogs />` is mounted
  at `app/src/shell.rs:292`, a sibling of `<main><Outlet /></main>` (`shell.rs:266`), and
  `AppShell` is the root `ParentRoute` view (`app/src/lib.rs:116`). The two `<Transition>`
  blocks (`collection.rs:279-301`, `:330-370`) live inside the page rendered through that
  `Outlet`. The dialog is several levels above them and above the route itself.
- **The dialog's state is shell-owned, not page-owned.** `provide_tree_manage()` is called at
  `app/src/shell.rs:187`; `delete_req` and `delete_open` are `RwSignal`s created there
  (`tree_manage.rs:242-243`, constructed at `:291-292`). They outlive any page, let alone a
  `Transition` re-render.
- **Nothing closes the dialog on navigation.** `delete_open` is read/written *only* inside
  `tree_manage.rs` (grep across `app/src`): set `true` by `open_delete` (`:343`) and `false` on
  a successful delete (`:571`), plus the `Dialog` primitive's own user-initiated dismissals
  (Escape / backdrop / `DialogClose`), which take `open` from `DialogContext`
  (`components/ui/dialog.rs:35-39, 58`). The only pathname-tracking effect in the shell
  (`shell.rs:198-201`) closes `rail_open`, not any dialog.
- **Disposal of the old header takes nothing the dialog needs.** When B's payload lands, the
  `<Transition>` at `:279-301` replaces the `CollectionHeader` subtree, disposing A's
  `StoredValue` (`collection.rs:651`) and `aim` `Callback` (`:659`). `DeleteReq` is fully-owned
  data — `Id`, `String`, `HashSet<Id>`, `Option<Id>`, `i64` (`tree_manage.rs:117-129`) — already
  copied out at open time, and `submit_delete`'s `navigate` / `pathname` hooks belong to
  `TreeDialogs`' own owner (`tree_manage.rs:458-459`, read in the component body precisely so
  "a callback that may run after a navigation" still works).
- **Net behavior:** the dialog stays mounted, stays open, keeps rendering A's name and A's
  counts, and confirming still deletes A. The page *behind* it re-renders as B. `submit_delete`
  then computes `route_after_delete(&pathname.get_untracked(), &req)` (`tree_manage.rs:565`)
  against the **live** pathname — which by then is B's. B is not in A's `subtree`, so
  `route_after_delete` returns `None` (`:158-160`), the page stays on B, and `revision` is
  bumped (`:586`). That is the correct outcome for "you are standing on B and deleted A".

## Open — needs a runtime check
None — both questions are settled statically; the display source and the mount point are
unambiguous in the code. An optional e2e confirmation, if one is ever wanted, would be: with
`view_res` for B stalled, click B in the sidebar, then immediately open the header kebab →
`Delete…` and assert the delete dialog's title (`[aria-label="Delete collection"] h3` — the
title is a bare `<h3>`, no `data-slot`/`data-testid`, `components/ui/dialog.rs:28`) reads
`Delete A?` while `[data-testid=collection-title]` still reads `A`; then let B land and assert
the dialog title is *still* `Delete A?` while `collection-title` has become `B`.
