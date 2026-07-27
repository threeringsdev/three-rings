//! Collection-tree management (specs/app-ui.md → Collection tree; the
//! "management" half of the two-task split): one shared context menu over
//! every tree row (create / rename / **move** / delete, each confirmed in a
//! dialog) and the commit half of the drag layer (reparent by dropping *onto* a
//! row, reorder by dropping on a row's edge band — fractional `position`
//! midpoints, specs/collection-api.md → Tree CRUD).
//!
//! The client pre-checks cycles only to paint drop targets; the API is the
//! cycle-guard terminus, and its rejections (409 on a cycle, on the Inbox)
//! surface as an error toast rather than being silently swallowed.
//!
//! **`Move to…` is the mouse-free half of the drag layer** (IA → My cards:
//! "create / rename / delete / move happens in place via context menus"). HTML5
//! drag fires on neither touch nor the keyboard, so without it a collection
//! cannot be moved at all on a phone or from the keyboard. It is a destination
//! picker over the same `command` list the catalog toolbar and the selection
//! tray use, and it commits through the very same
//! `reparent_collection` + `reorder_collection` adapters the drop does — see
//! [`plan_move`] for what it covers and [`move_destinations`] for the cycle
//! guard being enforced at the *source* rather than left to the server's 409.

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_location, use_navigate};
use shared::{CollectionKind, CollectionSummary, CollectionTreeRow, Id};

use super::tree::{find_node, subtree_ids, CollectionTreeResource, TreeNode};
use crate::catalog::destination::{picker_order, Destination, DestinationList, DestinationRow};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::context_menu::{ContextMenuContent, ContextMenuItem};
use crate::components::ui::dialog::{
    Dialog, DialogBody, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader,
    DialogTitle,
};
use crate::components::ui::input::Input;
use crate::components::ui::separator::Separator;
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};

/// The move picker's search field — a deterministic handle so the dialog can
/// put the caret in it on open. Without that the keyboard path dead-ends at an
/// opened dialog nothing is focused in.
pub const MOVE_INPUT_ID: &str = "tree-move-input";

/// What the shared context menu is aimed at: one row, or the rail
/// background (top-level create).
#[derive(Clone, PartialEq)]
pub enum MenuTarget {
    Row {
        id: Id,
        name: String,
        is_inbox: bool,
        /// Its current parent — `None` at the top level. The move picker marks
        /// this one ✓ and refuses it as a destination (it is where the row
        /// already is).
        parent_id: Option<Id>,
        /// Self + every descendant: the drop targets this row may not take and
        /// the destinations the move picker may not offer (the client-side
        /// cycle guard). Its `len() - 1` is the delete confirm's subtree count.
        forbidden: HashSet<Id>,
        /// Rolled-up present copies (for the delete confirm).
        cards: i64,
    },
    Background,
}

impl MenuTarget {
    /// Aim the shared menu at a collection named by the **route** rather than by
    /// a tree row — the collection-header `⋯` (`super::collection`).
    ///
    /// The identity half (`id` / `name` / `is_inbox` / `parent_id`) comes from
    /// the page's own payload, so the menu describes what the header describes.
    /// The `forbidden` set can only come from the tree, which is the only read
    /// that knows the whole subtree; when the tree does not contain the node —
    /// a collection created since the shell's fetch, or a failed tree read — the
    /// guard degrades to "not itself" and the server's recursive ancestor check
    /// is the terminus, exactly the standing a [`MoveReq`] snapshot has after a
    /// refetch lands under it.
    ///
    /// `cards` is passed in rather than read off the tree because the page has a
    /// better number: the view's own whole-collection totals, plus any stepper
    /// delta the header is already showing. A delete confirm that named a
    /// different count than the counts line two rows above it would be its own
    /// small lie.
    pub fn for_collection(c: &CollectionSummary, roots: &[TreeNode], cards: i64) -> MenuTarget {
        let mut forbidden = HashSet::new();
        match find_node(roots, c.id) {
            Some(node) => subtree_ids(node, &mut forbidden),
            None => {
                forbidden.insert(c.id);
            }
        }
        MenuTarget::Row {
            id: c.id,
            name: c.name.clone(),
            is_inbox: c.is_inbox,
            parent_id: c.parent_id,
            forbidden,
            cards,
        }
    }
}

/// A create dialog request: where and what kind.
#[derive(Clone, PartialEq)]
pub struct CreateReq {
    /// `None` = top level; `Some((id, name))` = inside that collection.
    pub parent: Option<(Id, String)>,
    pub kind: CollectionKind,
}

/// A delete confirm request — snapshotted when the dialog opens, so the
/// confirm can never target a *different* row than the one it named (the
/// shared `menu_target` keeps moving as the user right-clicks around).
#[derive(Clone, PartialEq)]
pub struct DeleteReq {
    pub id: Id,
    pub name: String,
    /// Self plus every descendant — the ids this delete cascades away. Carried
    /// (rather than just a count) because **the route may be inside it**: see
    /// [`route_after_delete`].
    pub subtree: HashSet<Id>,
    /// Where the deleted node lived, so a page standing on it knows where to
    /// go. `None` = top level.
    pub parent_id: Option<Id>,
    /// Rolled-up present copies that cascade with it.
    pub cards: i64,
}

impl DeleteReq {
    /// Nested collections that cascade with it (for the confirm copy) — the
    /// subtree minus the node itself.
    fn descendants(&self) -> usize {
        self.subtree.len().saturating_sub(1)
    }
}

/// Where the app must go once a delete succeeds, or `None` to stay put.
///
/// **Deleting the collection you are looking at is a real path**, and it became a
/// likely one the moment the collection header grew its own `⋯`: the kebab's
/// subject *is* the current route. Nothing refetches `/my/collections/:id` on a
/// delete, so without this the page keeps rendering a collection the database no
/// longer has — stale rows, a breadcrumb that has already dropped the node, and
/// a reload that 404s. The cascade makes it wider than the one node: every
/// descendant goes too, so `/my/collections/<a child>` is just as dead.
///
/// The destination is the deleted node's parent (which by construction is *not*
/// in the subtree), or `/my` at the top level — the same "back walks up the tree"
/// rule the mobile header's back link follows.
pub fn route_after_delete(pathname: &str, req: &DeleteReq) -> Option<String> {
    // `/my/collections/{id}` and its subpages (`…/needs`) — anything else is not
    // standing on a collection and has nothing to flee.
    let rest = pathname.strip_prefix("/my/collections/")?;
    let id = rest.split('/').next()?;
    let id = Id::parse_str(id).ok()?;
    if !req.subtree.contains(&id) {
        return None;
    }
    Some(match req.parent_id {
        Some(parent) => format!("/my/collections/{parent}"),
        None => "/my".to_string(),
    })
}

/// A move request — snapshotted when the picker opens, for the same reason
/// [`DeleteReq`] is: the shared `menu_target` keeps moving as the user
/// right-clicks around, and a picker that re-read it at commit time could move
/// a *different* collection than the one it named.
#[derive(Clone, PartialEq)]
pub struct MoveReq {
    pub id: Id,
    pub name: String,
    /// Where it lives now — marked ✓ in the picker, and a no-op if picked.
    pub parent_id: Option<Id>,
    /// Self + every descendant: the destinations the picker must not offer.
    pub forbidden: HashSet<Id>,
}

/// Where a `Move to…` pick puts the collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveTarget {
    /// Out to the top level (`parent_id = None`) — the one destination that is
    /// not a collection, and the reason the picker rows are [`DestinationRow`]s
    /// rather than `DestinationOption`s.
    TopLevel,
    Into(Id),
}

impl MoveTarget {
    fn parent(self) -> Option<Id> {
        match self {
            MoveTarget::TopLevel => None,
            MoveTarget::Into(id) => Some(id),
        }
    }
}

/// A live drag: the moved node plus the ids a drop may not target (itself
/// and every descendant — the client-side cycle pre-check).
#[derive(Clone, PartialEq)]
pub struct DragState {
    pub id: Id,
    pub parent_id: Option<Id>,
    pub forbidden: HashSet<Id>,
}

/// Where on a row a drag would land.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DropIntent {
    /// Above the row, among its siblings.
    Before,
    /// Into the row (reparent).
    Into,
    /// Below the row, among its siblings.
    After,
}

impl DropIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            DropIntent::Before => "before",
            DropIntent::Into => "into",
            DropIntent::After => "after",
        }
    }
}

/// Management state shared by the rows, the menu, and the dialogs.
#[derive(Clone, Copy)]
pub struct TreeManage {
    pub menu_target: RwSignal<Option<MenuTarget>>,
    pub drag: RwSignal<Option<DragState>>,
    pub drop_hint: RwSignal<Option<(Id, DropIntent)>>,
    create_req: RwSignal<Option<CreateReq>>,
    create_open: RwSignal<bool>,
    create_name: RwSignal<String>,
    rename_req: RwSignal<Option<(Id, String)>>,
    rename_open: RwSignal<bool>,
    rename_name: RwSignal<String>,
    delete_req: RwSignal<Option<DeleteReq>>,
    delete_open: RwSignal<bool>,
    move_req: RwSignal<Option<MoveReq>>,
    pub move_open: RwSignal<bool>,
    /// An op in flight (disables dialog submits).
    busy: RwSignal<bool>,
    /// Inline dialog error (server message) — cleared on open/submit.
    error: RwSignal<Option<String>>,
    /// Bumped after every successful create / rename / move. A page whose own
    /// read describes a collection takes it as a resource **source**, so a
    /// mutation refetches what it invalidated — the same structural trick
    /// `HoldingsRevision` plays for the tray's batch move.
    ///
    /// It exists because the tree refetch alone is not enough for the collection
    /// view, and the header kebab is what made that visible: `/my/collections/:id`
    /// reads its title, its counts and its **folder rows** from `collection_view`,
    /// none of which the tree read can update. Renaming the collection you are
    /// standing on left the `<h1>` on the old name while the breadcrumb beside it
    /// showed the new one; creating a child from its own header added a row that
    /// did not appear. Both read as "the action did nothing".
    ///
    /// Delete bumps it too, but **only when it does not navigate**. The two cases
    /// are genuinely different and the first cut of this got it wrong by treating
    /// them as one: when [`route_after_delete`] sends the page up to the parent,
    /// that remounts and refetches by itself, and bumping as well would refetch a
    /// collection that no longer exists — a stale page traded for an error one.
    /// When it returns `None` there is no navigation, and the delete still changed
    /// what this page says: a deleted child is one of its folder rows and part of
    /// its rollup.
    pub revision: RwSignal<u32>,
}

/// Provided by the **app shell**, not by the tree. ⌘K's `New binder…` /
/// `New deck…` open the create dialog below from anywhere — including Catalog
/// mode, where `CollectionTreeNav` isn't mounted — so the state has to outlive
/// the sidebar. [`TreeDialogs`] is mounted at the shell for the same reason
/// plus a sharper one: the sidebar is off-screen below `md`, and a dialog
/// cannot be shown from inside a hidden subtree.
pub fn provide_tree_manage() {
    provide_context(TreeManage {
        menu_target: RwSignal::new(None),
        drag: RwSignal::new(None),
        drop_hint: RwSignal::new(None),
        create_req: RwSignal::new(None),
        create_open: RwSignal::new(false),
        create_name: RwSignal::new(String::new()),
        rename_req: RwSignal::new(None),
        rename_open: RwSignal::new(false),
        rename_name: RwSignal::new(String::new()),
        delete_req: RwSignal::new(None),
        delete_open: RwSignal::new(false),
        move_req: RwSignal::new(None),
        move_open: RwSignal::new(false),
        busy: RwSignal::new(false),
        error: RwSignal::new(None),
        revision: RwSignal::new(0),
    })
}

impl TreeManage {
    pub fn open_create(&self, parent: Option<(Id, String)>, kind: CollectionKind) {
        self.create_req.set(Some(CreateReq { parent, kind }));
        self.create_name.set(String::new());
        self.error.set(None);
        self.create_open.set(true);
    }

    pub fn open_rename(&self, id: Id, current: String) {
        self.rename_req.set(Some((id, current.clone())));
        self.rename_name.set(current);
        self.error.set(None);
        self.rename_open.set(true);
    }

    /// Open the delete confirm, **snapshotting** its subject from the current
    /// `menu_target` — the confirm then targets this row even if a later
    /// right-click moves `menu_target` while the dialog is open. A no-op if the
    /// menu wasn't aimed at a row (the background menu has no delete).
    pub fn open_delete(&self) {
        let subject = match self.menu_target.get_untracked() {
            Some(MenuTarget::Row {
                id,
                name,
                parent_id,
                forbidden,
                cards,
                ..
            }) => DeleteReq {
                id,
                name,
                // Self *and* every descendant: the confirm counts the nested ones
                // (`descendants()`), and `route_after_delete` needs the whole set
                // because the route may be standing on any of them.
                subtree: forbidden,
                parent_id,
                cards,
            },
            _ => return,
        };
        self.delete_req.set(Some(subject));
        self.error.set(None);
        self.delete_open.set(true);
    }

    /// Open the move picker, snapshotting its subject the same way
    /// [`Self::open_delete`] does. A no-op off a row, and a no-op on the Inbox
    /// — the API refuses to reparent it at all (`AND NOT is_inbox`), so
    /// offering the action would only ever produce a 409.
    pub fn open_move(&self) {
        let subject = match self.menu_target.get_untracked() {
            Some(MenuTarget::Row {
                id,
                name,
                is_inbox: false,
                parent_id,
                forbidden,
                ..
            }) => MoveReq {
                id,
                name,
                parent_id,
                forbidden,
            },
            _ => return,
        };
        self.move_req.set(Some(subject));
        self.error.set(None);
        self.move_open.set(true);
    }
}

/// Strip the server-fn transport prefix so dialogs and toasts show the
/// `ApiError` message ("conflict: …"), not the wrapper.
fn user_msg(e: &ServerFnError<String>) -> String {
    match e {
        ServerFnError::ServerError(msg) => msg.clone(),
        other => other.to_string(),
    }
}

/// The context-menu panel, aimed by [`TreeManage::menu_target`].
#[component]
pub fn TreeMenu() -> impl IntoView {
    let manage = expect_context::<TreeManage>();

    view! {
        <ContextMenuContent class="w-56">
            {move || match manage.menu_target.get() {
                Some(MenuTarget::Row { id, name, is_inbox, .. }) => {
                    let parent = Some((id, name.clone()));
                    let parent2 = parent.clone();
                    let rename_name = name.clone();
                    view! {
                        <span class="text-muted-foreground block truncate px-2 py-1.5 text-xs">
                            {name.clone()}
                        </span>
                        <ContextMenuItem on_select=Callback::new(move |()| {
                            manage.open_create(parent.clone(), CollectionKind::Binder)
                        })>"New binder inside…"</ContextMenuItem>
                        <ContextMenuItem on_select=Callback::new(move |()| {
                            manage.open_create(parent2.clone(), CollectionKind::Deck)
                        })>"New deck inside…"</ContextMenuItem>
                        {(!is_inbox)
                            .then(|| {
                                view! {
                                    <Separator class="my-1" />
                                    // The mouse-free half of the drag layer
                                    // (module doc). Above Rename because it is
                                    // the action drag *also* offers.
                                    <ContextMenuItem on_select=Callback::new(move |()| {
                                        manage.open_move()
                                    })>"Move to…"</ContextMenuItem>
                                    <ContextMenuItem on_select=Callback::new(move |()| {
                                        manage.open_rename(id, rename_name.clone())
                                    })>"Rename…"</ContextMenuItem>
                                    <ContextMenuItem
                                        class="text-destructive hover:bg-destructive/10 hover:text-destructive"
                                        on_select=Callback::new(move |()| manage.open_delete())
                                    >
                                        "Delete…"
                                    </ContextMenuItem>
                                }
                            })}
                    }
                        .into_any()
                }
                Some(MenuTarget::Background) => view! {
                    <ContextMenuItem on_select=Callback::new(move |()| {
                        manage.open_create(None, CollectionKind::Binder)
                    })>"New binder…"</ContextMenuItem>
                    <ContextMenuItem on_select=Callback::new(move |()| {
                        manage.open_create(None, CollectionKind::Deck)
                    })>"New deck…"</ContextMenuItem>
                }
                    .into_any(),
                None => "".into_any(),
            }}
        </ContextMenuContent>
    }
}

/// The four management dialogs, mounted once — **by the app shell**, not by
/// the tree (see [`provide_tree_manage`]).
#[component]
pub fn TreeDialogs() -> impl IntoView {
    let manage = expect_context::<TreeManage>();
    let tree_res = expect_context::<CollectionTreeResource>();
    let tree = tree_res.0;
    // Read in the component body, not inside the move picker's `Suspend`.
    // Contexts do reach into a resolved async view here, but the tree already
    // paid for learning that the rule is subtle (a `Provider` *above* a
    // `Suspense` does not reach `use_context_menu()` inside it), and a body
    // read is unconditionally correct.
    let toast = expect_context::<ToastHandle>();
    // Read in the component body for the same reason: hooks belong to the
    // reactive owner, not to a callback that may run after a navigation.
    let navigate = use_navigate();
    let pathname = use_location().pathname;

    // **Whether the tree read behind the move picker failed** — the third
    // `DestinationList` consumer, and the one where the collapse did real damage.
    //
    // `move_rows` renders `⬆ Top level` unconditionally, so a failed tree read
    // left the dialog listing exactly that one row, with `CommandEmpty` silent
    // (its registry is non-empty) and no error line anywhere. The dialog then
    // asserted that root is the only place this collection can go, and offered a
    // reparent as the user's only move. That is worse than the two flattened
    // lists this task's first pass fixed, because it is a *write* on a false
    // picture — and it is reachable exactly when the rail is already showing
    // "Couldn't load collections": `/my/collections/:id` reads `collection_view`,
    // not the tree, so the page around it loads fine, and
    // `MenuTarget::for_collection` deliberately degrades `forbidden` to `{self}`
    // on a failed tree read, which keeps `Move to…` live.
    //
    // Effect-written, like the tray's: a resource read in plain render is
    // unresolved during SSR and resolved at hydration, and hydration claims the
    // server's text without rewriting it. Safe here because the dialog is behind
    // `move_open`, a client signal that is false on every server render.
    let load_failed = RwSignal::new(false);
    Effect::new(move |_| {
        let failed = matches!(tree.get(), Some(Some(Err(_))));
        if failed != load_failed.get_untracked() {
            load_failed.set(failed);
        }
    });

    let submit_create = move || {
        let Some(req) = manage.create_req.get_untracked() else {
            return;
        };
        let name = manage.create_name.get_untracked().trim().to_string();
        if name.is_empty() {
            manage.error.set(Some("Name is required.".into()));
            return;
        }
        if manage.busy.get_untracked() {
            return;
        }
        manage.busy.set(true);
        manage.error.set(None);
        spawn_local(async move {
            let parent_id = req.parent.as_ref().map(|(id, _)| *id);
            match crate::create_collection(parent_id, req.kind, name).await {
                Ok(_) => {
                    manage.busy.set(false);
                    manage.create_open.set(false);
                    tree.refetch();
                    // The new child is a folder row on its parent's page, which
                    // is a different read from the tree's — see `revision`.
                    manage.revision.update(|r| *r = r.wrapping_add(1));
                }
                Err(e) => {
                    manage.busy.set(false);
                    manage.error.set(Some(user_msg(&e)));
                }
            }
        });
    };

    let submit_rename = move || {
        let Some((id, _)) = manage.rename_req.get_untracked() else {
            return;
        };
        let name = manage.rename_name.get_untracked().trim().to_string();
        if name.is_empty() {
            manage.error.set(Some("Name is required.".into()));
            return;
        }
        if manage.busy.get_untracked() {
            return;
        }
        manage.busy.set(true);
        manage.error.set(None);
        spawn_local(async move {
            match crate::rename_collection(id, name).await {
                Ok(_) => {
                    manage.busy.set(false);
                    manage.rename_open.set(false);
                    tree.refetch();
                    // The collection view's own `<h1>` and breadcrumb fallback
                    // come from `collection_view`, not from the tree.
                    manage.revision.update(|r| *r = r.wrapping_add(1));
                }
                Err(e) => {
                    manage.busy.set(false);
                    manage.error.set(Some(user_msg(&e)));
                }
            }
        });
    };

    let submit_delete = move || {
        // The snapshot taken when the dialog opened — never the live
        // `menu_target`, which a later right-click may have moved.
        let Some(req) = manage.delete_req.get_untracked() else {
            return;
        };
        if manage.busy.get_untracked() {
            return;
        }
        manage.busy.set(true);
        manage.error.set(None);
        // Decided *before* the await, off the route the confirm was answered on.
        let leaving = route_after_delete(&pathname.get_untracked(), &req);
        let navigate = navigate.clone();
        spawn_local(async move {
            match crate::delete_collection(req.id).await {
                Ok(()) => {
                    manage.busy.set(false);
                    manage.delete_open.set(false);
                    tree.refetch();
                    match leaving {
                        // Deleting the collection you are standing on (or an
                        // ancestor of it) leaves the page rendering a dead id —
                        // `route_after_delete` sends it up to the parent, which
                        // remounts the page and refetches on its own.
                        Some(to) => navigate(&to, Default::default()),
                        // Deleting anything *else* still changes what this page
                        // says. Standing on a parent and deleting one of its
                        // children is the likeliest case — the child is a folder
                        // row here (`view.children`) and its copies are in this
                        // header's rollup, and neither comes from the tree read.
                        // Without the bump the row stayed, linking to an id the
                        // database no longer had.
                        None => manage.revision.update(|r| *r = r.wrapping_add(1)),
                    }
                }
                Err(e) => {
                    manage.busy.set(false);
                    manage.error.set(Some(user_msg(&e)));
                }
            }
        });
    };

    let error_line = move || {
        manage.error.get().map(|msg| {
            view! { <p class="text-destructive text-sm" data-tree-dialog-error>{msg}</p> }
        })
    };

    // Delete-confirm copy, from the snapshot taken when the dialog opened.
    let delete_subject = move || {
        manage
            .delete_req
            .get()
            .map(|r| (r.name.clone(), r.descendants(), r.cards))
    };

    // Put the caret in the move picker's field when it opens. Without this the
    // keyboard path dead-ends: the menu item opens a dialog nothing is focused
    // in, so ↑↓/⏎ reach no row. Same shape (and same fallback timeout) as the
    // ⌘K palette's, because the field only exists on the mount that just
    // happened.
    Effect::new(move |_| {
        if manage.move_open.get() {
            #[cfg(feature = "hydrate")]
            {
                focus_move_field();
                // …and again a macrotask later, unconditionally. The field may
                // not be in the document yet on this pass (the `Show` below
                // renders in the same flush), and the context menu that
                // launched this dialog is also settling its own focus in that
                // flush. A later pass is the one that has to win.
                set_timeout(
                    || {
                        focus_move_field();
                    },
                    std::time::Duration::from_millis(0),
                );
            }
        }
    });

    view! {
        <Dialog id="tree-create" open=manage.create_open>
            <DialogContent aria_label="Create collection">
                <DialogBody>
                    <DialogHeader>
                        <DialogTitle>
                            {move || {
                                let kind = manage
                                    .create_req
                                    .get()
                                    .map(|r| r.kind)
                                    .unwrap_or(CollectionKind::Binder);
                                match kind {
                                    CollectionKind::Binder => "New binder",
                                    CollectionKind::Deck => "New deck",
                                }
                            }}
                        </DialogTitle>
                        <DialogDescription>
                            {move || match manage.create_req.get().and_then(|r| r.parent) {
                                Some((_, parent)) => format!("Inside {parent}."),
                                None => "At the top level.".to_string(),
                            }}
                        </DialogDescription>
                    </DialogHeader>
                    <form on:submit=move |ev| {
                        ev.prevent_default();
                        submit_create();
                    }>
                        <Input
                            id="tree-create-name"
                            placeholder="Name"
                            bind_value=manage.create_name
                        />
                    </form>
                    {error_line}
                    <DialogFooter>
                        <DialogClose>"Cancel"</DialogClose>
                        <Button
                            attr:id="tree-create-confirm"
                            attr:disabled=move || manage.busy.get()
                            on:click=move |_| submit_create()
                        >
                            "Create"
                        </Button>
                    </DialogFooter>
                </DialogBody>
            </DialogContent>
        </Dialog>

        <Dialog id="tree-rename" open=manage.rename_open>
            <DialogContent aria_label="Rename collection">
                <DialogBody>
                    <DialogHeader>
                        <DialogTitle>"Rename"</DialogTitle>
                        <DialogDescription>
                            {move || {
                                manage
                                    .rename_req
                                    .get()
                                    .map(|(_, current)| format!("Renaming {current}."))
                                    .unwrap_or_default()
                            }}
                        </DialogDescription>
                    </DialogHeader>
                    <form on:submit=move |ev| {
                        ev.prevent_default();
                        submit_rename();
                    }>
                        <Input
                            id="tree-rename-name"
                            placeholder="Name"
                            bind_value=manage.rename_name
                        />
                    </form>
                    {error_line}
                    <DialogFooter>
                        <DialogClose>"Cancel"</DialogClose>
                        <Button
                            attr:id="tree-rename-confirm"
                            attr:disabled=move || manage.busy.get()
                            on:click=move |_| submit_rename()
                        >
                            "Rename"
                        </Button>
                    </DialogFooter>
                </DialogBody>
            </DialogContent>
        </Dialog>

        <Dialog id="tree-delete" open=manage.delete_open>
            <DialogContent aria_label="Delete collection">
                <DialogBody>
                    <DialogHeader>
                        <DialogTitle>
                            {move || {
                                delete_subject()
                                    .map(|(name, _, _)| format!("Delete {name}?"))
                                    .unwrap_or_else(|| "Delete?".to_string())
                            }}
                        </DialogTitle>
                        <DialogDescription>
                            {move || {
                                delete_subject()
                                    .map(|(_, descendants, cards)| {
                                        let mut what = String::new();
                                        if descendants > 0 {
                                            what.push_str(&format!(
                                                "{descendants} nested collection{} and ",
                                                if descendants == 1 { "" } else { "s" },
                                            ));
                                        }
                                        what.push_str(&format!(
                                            "{cards} card{}",
                                            if cards == 1 { "" } else { "s" },
                                        ));
                                        format!(
                                            "This permanently deletes {what} inside it. This cannot be undone.",
                                        )
                                    })
                                    .unwrap_or_default()
                            }}
                        </DialogDescription>
                    </DialogHeader>
                    {error_line}
                    <DialogFooter>
                        <DialogClose>"Cancel"</DialogClose>
                        <Button
                            variant=ButtonVariant::Destructive
                            attr:id="tree-delete-confirm"
                            attr:disabled=move || manage.busy.get()
                            on:click=move |_| submit_delete()
                        >
                            "Delete"
                        </Button>
                    </DialogFooter>
                </DialogBody>
            </DialogContent>
        </Dialog>

        <Dialog id="tree-move" open=manage.move_open>
            <DialogContent aria_label="Move collection">
                <DialogBody>
                    <DialogHeader>
                        <DialogTitle>
                            {move || {
                                manage
                                    .move_req
                                    .get()
                                    .map(|r| format!("Move {}", r.name))
                                    .unwrap_or_else(|| "Move".to_string())
                            }}
                        </DialogTitle>
                        <DialogDescription>
                            "Choose where it lives. Anything inside it moves along, and it lands last among its new siblings."
                        </DialogDescription>
                    </DialogHeader>
                    // Mounted only while open, for the reason the palette's rows
                    // are: a closed dialog keeps its box in the DOM, so leaving
                    // the rows mounted would register N `command` items (and
                    // duplicate the `destination-option` seam) behind a closed
                    // overlay on every My-cards page.
                    <Show when=move || manage.move_open.get()>
                        <div class="overflow-hidden rounded-md border" data-testid="tree-move-list">
                            <DestinationList
                                placeholder="Search collections…"
                                empty="No collection to move into."
                                failed=load_failed
                                input_id=Some(MOVE_INPUT_ID.to_string())
                            >
                                // Same boundary the catalog picker and the tray
                                // use: the rows come off a resource, and only a
                                // suspense boundary keeps SSR and hydration in
                                // step with it.
                                <Transition fallback=|| {
                                    view! {
                                        <p class="text-muted-foreground p-3 text-sm">
                                            "Loading collections…"
                                        </p>
                                    }
                                }>
                                    {move || {
                                        // Read the snapshot *outside* the async
                                        // block so a second `Move to…` on a
                                        // different row rebuilds these rows.
                                        let req = manage.move_req.get();
                                        Suspend::new(async move {
                                            let Some(req) = req else {
                                                return ().into_any();
                                            };
                                            let rows = match tree.await {
                                                Some(Ok(dto)) => dto.collections,
                                                _ => Vec::new(),
                                            };
                                            move_rows(manage, tree_res, toast, req, rows).into_any()
                                        })
                                    }}
                                </Transition>
                            </DestinationList>
                        </div>
                    </Show>
                    {error_line}
                    <DialogFooter>
                        <DialogClose>"Cancel"</DialogClose>
                    </DialogFooter>
                </DialogBody>
            </DialogContent>
        </Dialog>
    }
}

/// The move picker's rows: `Top level` first, then every offerable collection.
///
/// **Built in one pass, in the order they are drawn, off data that was already
/// sorted** — which is what keeps `command`'s registry honest here.
/// `CommandItem` registers from its component body (view *construction*), and
/// `visible_ids()` returns registration order, so a consumer is safe exactly
/// when construction order equals document order. This one is: the `Top level`
/// row is unconditional and always first, the rest come out of
/// [`move_destinations`] pre-sorted, and typing only *hides* rows (the
/// primitive's per-item `is_visible` memo) — it never reorders them. Same
/// standing as the destination picker, unlike ⌘K which ranks and therefore has
/// to remount.
fn move_rows(
    manage: TreeManage,
    tree: CollectionTreeResource,
    toast: ToastHandle,
    req: MoveReq,
    rows: Vec<CollectionTreeRow>,
) -> impl IntoView {
    // `⬆ Top level` is rendered unconditionally, and stays that way when the tree
    // read failed: reparenting to root is the one destination that does not need
    // the tree to name it, so it is the same `fallback_rows` discipline the `/my`
    // root list follows. What it must not do is stand there *alone and unexplained*
    // — with `TreeDialogs`' `load_failed` wired in, the list now says the rest is
    // missing above it instead of implying root is the only place to go.
    let at_top = req.parent_id.is_none();
    let current = req.parent_id;
    let pick =
        move |target: MoveTarget| Callback::new(move |()| commit_move(tree, toast, manage, target));

    view! {
        <DestinationRow
            label="⬆ Top level"
            value="Top level"
            chosen=Signal::derive(move || at_top)
            on_select=pick(MoveTarget::TopLevel)
        />
        {move_destinations(&rows, &req.forbidden)
            .into_iter()
            .map(|c| {
                let is_current = current == Some(c.id);
                let value = c.name.clone();
                // The same `📥`/`🗂` label the other two pickers show — one
                // rule for what a destination reads as.
                let label = Destination {
                    id: c.id,
                    name: c.name,
                    is_inbox: c.is_inbox,
                }
                    .label();
                view! {
                    <DestinationRow
                        label=label
                        value=value
                        chosen=Signal::derive(move || is_current)
                        on_select=pick(MoveTarget::Into(c.id))
                    />
                }
            })
            .collect_view()}
    }
}

/// Focus the move picker's search field; `false` when the node isn't in the
/// document yet.
#[cfg(feature = "hydrate")]
fn focus_move_field() -> bool {
    use leptos::wasm_bindgen::JsCast;
    document()
        .get_element_by_id(MOVE_INPUT_ID)
        .and_then(|el| el.dyn_into::<leptos::web_sys::HtmlElement>().ok())
        .map(|el| {
            let _ = el.focus();
            true
        })
        .unwrap_or(false)
}

/// Commit a drop: `Into` reparents; an edge band reorders among the target's
/// siblings (reparenting first when they differ). Sibling positions come from
/// the flat server rows — the render order pins the Inbox first, but
/// `position` math must follow the server's (position, name) order.
pub fn commit_drop(
    tree: CollectionTreeResource,
    toast: ToastHandle,
    manage: TreeManage,
    drag: DragState,
    target_id: Id,
    intent: DropIntent,
) {
    let Some(Some(Ok(dto))) = tree.0.get_untracked() else {
        return;
    };
    let Some((new_parent, position)) = plan_drop(&dto.collections, &drag, target_id, intent) else {
        return;
    };

    // A cross-parent reorder is two writes (reparent, then set position) — the
    // trait has no combined op. They can't be one transaction from here, so the
    // toast is written to match what actually landed: if the reparent succeeds
    // but the position write fails, the collection *did* move to the new parent
    // (only its order among siblings is off), so we must not claim it didn't.
    let needs_reparent = new_parent != drag.parent_id;
    spawn_local(async move {
        let mut reparented = false;
        let mut result: Result<(), ServerFnError<String>> = Ok(());
        if needs_reparent {
            result = crate::reparent_collection(drag.id, new_parent).await;
            reparented = result.is_ok();
        }
        if result.is_ok() {
            if let Some(position) = position {
                result = crate::reorder_collection(drag.id, position).await;
            }
        }
        if let Err(e) = result {
            let msg = if reparented {
                // The move landed; only the ordering write failed.
                format!("Moved, but couldn't set its order: {}", user_msg(&e))
            } else {
                format!("Couldn't move: {}", user_msg(&e))
            };
            toast.show(ToastOptions::message(msg).kind(ToastKind::Error));
        }
        // Refetch either way — on failure the tree may have changed under
        // us (a stale render is exactly how a cycle slips past the
        // pre-check), and the sidebar must show the server's truth.
        tree.0.refetch();
        // Every tree mutation bumps it, this one included: a drag reparent moves
        // a folder row off one collection's page and onto another's, and neither
        // page learns that from the tree read. See `TreeManage::revision`.
        manage.revision.update(|r| *r = r.wrapping_add(1));
    });
}

/// Pure planner behind [`commit_drop`]: given the flat server rows (already in
/// `(position, name)` order per sibling group) and a drop, return the writes to
/// make — `(new_parent, Some(position))` for a reorder, `(new_parent, None)`
/// for a pure reparent, or `None` for a no-op (forbidden target, unknown ids,
/// or an `Into` where nothing changes). Split out so the fractional-index math
/// is unit-testable without a reactive graph.
fn plan_drop(
    rows: &[CollectionTreeRow],
    drag: &DragState,
    target_id: Id,
    intent: DropIntent,
) -> Option<(Option<Id>, Option<f64>)> {
    if drag.forbidden.contains(&target_id) {
        return None;
    }
    let target = rows.iter().find(|r| r.summary.id == target_id)?;

    match intent {
        DropIntent::Into => {
            if drag.parent_id == Some(target_id) {
                return None; // Already there.
            }
            Some((Some(target_id), None))
        }
        DropIntent::Before | DropIntent::After => {
            let new_parent = target.summary.parent_id;
            // Siblings in the destination group, in server order, excluding the
            // dragged node itself (it may already be a sibling here).
            let sibs: Vec<(Id, f64)> = rows
                .iter()
                .filter(|r| r.summary.parent_id == new_parent && r.summary.id != drag.id)
                .map(|r| (r.summary.id, r.summary.position))
                .collect();
            let ti = sibs.iter().position(|(id, _)| *id == target_id)?;
            let (lo, hi) = match intent {
                DropIntent::Before => ((ti > 0).then(|| sibs[ti - 1].1), Some(sibs[ti].1)),
                _ => (Some(sibs[ti].1), sibs.get(ti + 1).map(|(_, p)| *p)),
            };
            let position = match (lo, hi) {
                (Some(a), Some(b)) => (a + b) / 2.0,
                (None, Some(b)) => b - 1.0,
                (Some(a), None) => a + 1.0,
                (None, None) => 1.0,
            };
            Some((new_parent, Some(position)))
        }
    }
}

/// The collections `Move to…` may offer as a destination, in the picker's
/// order (Inbox pinned, then by name — [`picker_order`], the same order the
/// catalog toolbar and the tray use).
///
/// **The cycle guard is enforced here, at the source.** `forbidden` is the
/// moved node plus every descendant, so its own subtree is never on the list
/// and the picker cannot produce a request the server would 409. That mirrors
/// the drag path's client-first rule rather than leaning on the backstop: a
/// list that offers a destination and then errors is a worse answer than one
/// that never offers it.
///
/// The **Inbox is offerable**, and deliberately: the server rejects the Inbox
/// as a reparent *subject* (`AND NOT is_inbox` on the id), never as a *target*,
/// and dropping a collection *into* the Inbox is already legal by drag
/// (`drop_intent` collapses the Inbox's bands to `Into`). Withholding it here
/// would make the two paths disagree.
pub fn move_destinations(
    rows: &[CollectionTreeRow],
    forbidden: &HashSet<Id>,
) -> Vec<CollectionSummary> {
    picker_order(
        rows.iter()
            .filter(|r| !forbidden.contains(&r.summary.id))
            .map(|r| r.summary.clone())
            .collect(),
    )
}

/// Pure planner behind [`commit_move`]: the writes a `Move to…` pick makes, or
/// `None` for a no-op.
///
/// **It covers reparenting, and it lands the collection *last* among its new
/// siblings** — which is why it uses `reorder_collection` as well as
/// `reparent_collection`. A bare reparent carries the node's old `position`
/// into the new sibling group, where it lands wherever that number happens to
/// fall (or ties, broken by name); the drag path's `Into` has that same
/// ambiguity. Naming the landing spot is what makes the picker's outcome
/// predictable without a second "and where?" step.
///
/// **What it does not cover: reordering among siblings you are already among.**
/// Picking the parent a collection is already in is a no-op here, so moving a
/// row up or down within one group is still drag-only. Queued as follow-up
/// rather than guessed at — an ordering UI is a design question (a second
/// picker step? per-row up/down?) this task has no wireframe for.
fn plan_move(
    rows: &[CollectionTreeRow],
    req: &MoveReq,
    target: MoveTarget,
) -> Option<(Option<Id>, Option<f64>)> {
    let new_parent = target.parent();
    if let Some(parent) = new_parent {
        // The picker never offers these; refused again here because the
        // snapshot outlives the list it was built from (a refetch can land
        // while the dialog is open).
        if req.forbidden.contains(&parent) {
            return None;
        }
        rows.iter().find(|r| r.summary.id == parent)?;
    }
    if new_parent == req.parent_id {
        return None; // Already there.
    }
    // Last among the new siblings — the moved node itself excluded, since a
    // reparent within the same group is not a case that reaches here but a
    // stale row set could still contain it.
    let last = rows
        .iter()
        .filter(|r| r.summary.parent_id == new_parent && r.summary.id != req.id)
        .map(|r| r.summary.position)
        .fold(None::<f64>, |acc, p| Some(acc.map_or(p, |a| a.max(p))));
    Some((new_parent, Some(last.map_or(1.0, |p| p + 1.0))))
}

/// Commit a `Move to…` pick through the same two adapters the drop uses.
///
/// The failure wording follows [`commit_drop`]'s, and for its reason: the two
/// writes are not one transaction, so a reparent that lands and an order write
/// that doesn't must not be reported as "couldn't move" — it *did* move.
/// Success and that partial case both close the dialog (the collection is where
/// the user asked); an outright failure keeps it open with the server's message
/// inline, like the other three dialogs.
pub fn commit_move(
    tree: CollectionTreeResource,
    toast: ToastHandle,
    manage: TreeManage,
    target: MoveTarget,
) {
    let Some(req) = manage.move_req.get_untracked() else {
        return;
    };
    let Some(Some(Ok(dto))) = tree.0.get_untracked() else {
        return;
    };
    let Some((new_parent, position)) = plan_move(&dto.collections, &req, target) else {
        // Picking where it already lives is a legitimate "never mind".
        manage.move_open.set(false);
        return;
    };
    if manage.busy.get_untracked() {
        return;
    }
    manage.busy.set(true);
    manage.error.set(None);
    spawn_local(async move {
        let mut reparented = false;
        let mut result = crate::reparent_collection(req.id, new_parent).await;
        if result.is_ok() {
            reparented = true;
            if let Some(position) = position {
                result = crate::reorder_collection(req.id, position).await;
            }
        }
        manage.busy.set(false);
        match (result, reparented) {
            (Ok(()), _) => manage.move_open.set(false),
            (Err(e), true) => {
                // The move landed; only the ordering write failed.
                manage.move_open.set(false);
                toast.show(
                    ToastOptions::message(format!(
                        "Moved, but couldn't set its order: {}",
                        user_msg(&e)
                    ))
                    .kind(ToastKind::Error),
                );
            }
            (Err(e), false) => manage.error.set(Some(user_msg(&e))),
        }
        // Refetch either way — on failure the tree may have changed under us,
        // and the sidebar must show the server's truth.
        tree.0.refetch();
        // A move changes the moved collection's `parent_id`, which is what the
        // header kebab's next `Move to…` snapshots and what the breadcrumb walks
        // — and both a page *on* the moved node and a page on its old or new
        // parent describe a different set of folder rows now.
        manage.revision.update(|r| *r = r.wrapping_add(1));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::CollectionKind;

    fn row(id: u128, parent: Option<u128>, name: &str, position: f64) -> CollectionTreeRow {
        CollectionTreeRow {
            summary: CollectionSummary {
                id: Id::from_u128(id),
                parent_id: parent.map(Id::from_u128),
                kind: CollectionKind::Binder,
                name: name.into(),
                is_inbox: false,
                position,
                format: None,
            },
            present: 0,
        }
    }

    fn drag(id: u128, parent: Option<u128>, forbidden: &[u128]) -> DragState {
        DragState {
            id: Id::from_u128(id),
            parent_id: parent.map(Id::from_u128),
            forbidden: forbidden.iter().map(|&i| Id::from_u128(i)).collect(),
        }
    }

    // Three top-level siblings A(1) B(2) C(3); drag a fourth node D.
    fn top_level() -> Vec<CollectionTreeRow> {
        vec![
            row(1, None, "A", 1.0),
            row(2, None, "B", 2.0),
            row(3, None, "C", 3.0),
            row(9, None, "D", 4.0),
        ]
    }

    #[test]
    fn into_reparents_without_a_position() {
        let plan = plan_drop(
            &top_level(),
            &drag(9, None, &[9]),
            Id::from_u128(1),
            DropIntent::Into,
        );
        assert_eq!(plan, Some((Some(Id::from_u128(1)), None)));
    }

    #[test]
    fn into_own_current_parent_is_a_noop() {
        // D already sits under B; dropping D into B changes nothing.
        let rows = vec![row(2, None, "B", 2.0), row(9, Some(2), "D", 1.0)];
        assert_eq!(
            plan_drop(
                &rows,
                &drag(9, Some(2), &[9]),
                Id::from_u128(2),
                DropIntent::Into
            ),
            None
        );
    }

    #[test]
    fn before_first_sibling_goes_below_it() {
        // Before A(1): no lower neighbor → A.position - 1.0.
        let plan = plan_drop(
            &top_level(),
            &drag(9, None, &[9]),
            Id::from_u128(1),
            DropIntent::Before,
        );
        assert_eq!(plan, Some((None, Some(0.0))));
    }

    #[test]
    fn before_middle_sibling_is_the_midpoint() {
        // Before B(2): between A(1) and B(2) → 1.5.
        let plan = plan_drop(
            &top_level(),
            &drag(9, None, &[9]),
            Id::from_u128(2),
            DropIntent::Before,
        );
        assert_eq!(plan, Some((None, Some(1.5))));
    }

    #[test]
    fn after_last_sibling_goes_above_it() {
        // After C(3): no upper neighbor → C.position + 1.0.
        let plan = plan_drop(
            &top_level(),
            &drag(9, None, &[9]),
            Id::from_u128(3),
            DropIntent::After,
        );
        assert_eq!(plan, Some((None, Some(4.0))));
    }

    #[test]
    fn reorder_skips_the_dragged_node_when_computing_neighbors() {
        // Drag B(2) itself to After A(1): B is excluded from `sibs`, so the
        // neighbors are A(1) and C(3) → midpoint 2.0, not A/B.
        let plan = plan_drop(
            &top_level(),
            &drag(2, None, &[2]),
            Id::from_u128(1),
            DropIntent::After,
        );
        assert_eq!(plan, Some((None, Some(2.0))));
    }

    #[test]
    fn forbidden_target_is_rejected() {
        // Can't drop a node onto itself or a descendant.
        assert_eq!(
            plan_drop(
                &top_level(),
                &drag(9, None, &[9, 1]),
                Id::from_u128(1),
                DropIntent::Into
            ),
            None
        );
    }

    #[test]
    fn unknown_target_is_rejected() {
        assert_eq!(
            plan_drop(
                &top_level(),
                &drag(9, None, &[9]),
                Id::from_u128(404),
                DropIntent::Into
            ),
            None
        );
    }

    // ------------------------------------------------- Move to… (picker) --

    fn named(id: u128, parent: Option<u128>, name: &str, position: f64) -> CollectionTreeRow {
        row(id, parent, name, position)
    }

    fn inbox(id: u128) -> CollectionTreeRow {
        let mut r = row(id, None, "Inbox", 0.0);
        r.summary.is_inbox = true;
        r
    }

    fn req(id: u128, parent: Option<u128>, forbidden: &[u128]) -> MoveReq {
        MoveReq {
            id: Id::from_u128(id),
            name: "Moved".into(),
            parent_id: parent.map(Id::from_u128),
            forbidden: forbidden.iter().map(|&i| Id::from_u128(i)).collect(),
        }
    }

    fn offered(rows: &[CollectionTreeRow], forbidden: &[u128]) -> Vec<String> {
        let set: HashSet<Id> = forbidden.iter().map(|&i| Id::from_u128(i)).collect();
        move_destinations(rows, &set)
            .into_iter()
            .map(|c| c.name)
            .collect()
    }

    /// Inbox, Shoebox > Rares, Trade, plus the node being moved (9) under Trade.
    fn nested() -> Vec<CollectionTreeRow> {
        vec![
            inbox(4),
            named(1, None, "Shoebox", 1.0),
            named(2, Some(1), "Rares", 1.0),
            named(3, None, "Trade", 2.0),
            named(9, Some(3), "Moved", 1.0),
            named(10, Some(9), "Inside Moved", 1.0),
        ]
    }

    #[test]
    fn the_picker_never_offers_the_moved_nodes_own_subtree() {
        // The cycle guard at the *source*: 9 and its child 10 are gone from the
        // list, so no pick can produce the 409 the server would answer.
        let names = offered(&nested(), &[9, 10]);
        assert!(!names.iter().any(|n| n == "Moved" || n == "Inside Moved"));
        // …and the positive control: everything else is still on offer, so the
        // exclusion is the subtree and not the whole list.
        assert_eq!(names, ["Inbox", "Rares", "Shoebox", "Trade"]);
    }

    #[test]
    fn the_inbox_is_an_offerable_destination() {
        // The API's `AND NOT is_inbox` guards the reparent *subject*, never the
        // target, and dropping *into* the Inbox is already legal by drag — so
        // withholding it here would make the two paths disagree. Pinned first,
        // per `picker_order`.
        assert_eq!(offered(&nested(), &[9, 10])[0], "Inbox");
    }

    #[test]
    fn a_move_lands_last_among_its_new_siblings() {
        // Shoebox already holds Rares(1.0); 9 lands after it.
        let plan = plan_move(
            &nested(),
            &req(9, Some(3), &[9, 10]),
            MoveTarget::Into(Id::from_u128(1)),
        );
        assert_eq!(plan, Some((Some(Id::from_u128(1)), Some(2.0))));
    }

    #[test]
    fn a_move_into_an_empty_collection_seeds_position_one() {
        let plan = plan_move(
            &nested(),
            &req(9, Some(3), &[9, 10]),
            MoveTarget::Into(Id::from_u128(2)),
        );
        assert_eq!(plan, Some((Some(Id::from_u128(2)), Some(1.0))));
    }

    #[test]
    fn a_move_to_top_level_lands_after_the_last_root() {
        // Roots are Inbox(0.0), Shoebox(1.0), Trade(2.0) → 3.0.
        let plan = plan_move(&nested(), &req(9, Some(3), &[9, 10]), MoveTarget::TopLevel);
        assert_eq!(plan, Some((None, Some(3.0))));
    }

    #[test]
    fn picking_the_parent_it_already_has_is_a_no_op() {
        // Both directions of "already there" — the ✓ row is pickable, and it
        // must not fire two writes that change nothing.
        assert_eq!(
            plan_move(
                &nested(),
                &req(9, Some(3), &[9, 10]),
                MoveTarget::Into(Id::from_u128(3))
            ),
            None
        );
        assert_eq!(
            plan_move(&nested(), &req(1, None, &[1, 2]), MoveTarget::TopLevel),
            None
        );
    }

    #[test]
    fn a_forbidden_or_unknown_destination_plans_nothing() {
        // The picker never offers these, but the snapshot outlives the list it
        // was built from — a refetch can land while the dialog is open.
        assert_eq!(
            plan_move(
                &nested(),
                &req(9, Some(3), &[9, 10]),
                MoveTarget::Into(Id::from_u128(10))
            ),
            None,
            "its own descendant"
        );
        assert_eq!(
            plan_move(
                &nested(),
                &req(9, Some(3), &[9, 10]),
                MoveTarget::Into(Id::from_u128(404))
            ),
            None,
            "a collection that is no longer there"
        );
    }

    // ------------------------------------- the collection header's subject --

    fn summary(id: u128, parent: Option<u128>, name: &str, is_inbox: bool) -> CollectionSummary {
        CollectionSummary {
            id: Id::from_u128(id),
            parent_id: parent.map(Id::from_u128),
            kind: CollectionKind::Binder,
            name: name.into(),
            is_inbox,
            position: 1.0,
            format: None,
        }
    }

    /// The `nested()` cast as the tree the *rail* renders: Inbox, Shoebox >
    /// Rares, Trade > Moved > Inside Moved.
    fn roots() -> Vec<TreeNode> {
        crate::my::tree::assemble(shared::CollectionTree {
            collections: nested(),
            shopping_short: 0,
        })
        .roots
    }

    fn row_parts(t: &MenuTarget) -> (Id, &str, bool, Option<Id>, Vec<u128>, i64) {
        match t {
            MenuTarget::Row {
                id,
                name,
                is_inbox,
                parent_id,
                forbidden,
                cards,
            } => {
                let mut ids: Vec<u128> = forbidden.iter().map(|i| i.as_u128()).collect();
                ids.sort_unstable();
                (*id, name.as_str(), *is_inbox, *parent_id, ids, *cards)
            }
            MenuTarget::Background => panic!("the header always aims at a row"),
        }
    }

    #[test]
    fn the_header_subject_carries_the_whole_subtree_from_the_tree() {
        // `Moved`(9) holds `Inside Moved`(10). Both are forbidden destinations
        // for a `Move to…` opened from *its own page*, exactly as they are from
        // its tree row — the cycle guard cannot be weaker on the second surface.
        let t = MenuTarget::for_collection(&summary(9, Some(3), "Moved", false), &roots(), 42);
        let (id, name, is_inbox, parent, forbidden, cards) = row_parts(&t);
        assert_eq!(id, Id::from_u128(9));
        assert_eq!(name, "Moved");
        assert!(!is_inbox);
        assert_eq!(parent, Some(Id::from_u128(3)));
        assert_eq!(forbidden, [9, 10]);
        assert_eq!(cards, 42);
    }

    #[test]
    fn a_collection_the_tree_has_not_seen_still_gets_a_menu() {
        // Just created, or the tree read failed: the subtree is unknowable, so
        // the guard degrades to "not itself" and the server's ancestor check is
        // the terminus. The menu must still open — a header with no actions is
        // worse than one whose picker can be told no.
        let t = MenuTarget::for_collection(&summary(404, None, "Fresh", false), &roots(), 0);
        let (_, _, _, parent, forbidden, _) = row_parts(&t);
        assert_eq!(parent, None);
        assert_eq!(forbidden, [404]);
    }

    #[test]
    fn the_inbox_route_is_marked_as_the_inbox() {
        // `/my/collections/:id` can be an Inbox id, and the menu withholds
        // move/rename/delete on that flag — all three are refused by the server
        // (`AND NOT is_inbox` on rename, delete and reparent alike).
        let t = MenuTarget::for_collection(&summary(4, None, "Inbox", true), &roots(), 7);
        let (_, name, is_inbox, _, forbidden, _) = row_parts(&t);
        assert_eq!(name, "Inbox");
        assert!(is_inbox);
        // Nothing is nested under it here, so it forbids only itself.
        assert_eq!(forbidden, [4]);
    }

    // ------------------------------------- deleting what you are looking at --

    fn del(id: u128, parent: Option<u128>, subtree: &[u128]) -> DeleteReq {
        DeleteReq {
            id: Id::from_u128(id),
            name: "Doomed".into(),
            subtree: subtree.iter().map(|&i| Id::from_u128(i)).collect(),
            parent_id: parent.map(Id::from_u128),
            cards: 0,
        }
    }

    fn path_of(id: u128) -> String {
        format!("/my/collections/{}", Id::from_u128(id))
    }

    #[test]
    fn deleting_the_collection_you_are_viewing_goes_up_to_its_parent() {
        let req = del(9, Some(3), &[9, 10]);
        assert_eq!(
            route_after_delete(&path_of(9), &req).as_deref(),
            Some(path_of(3).as_str())
        );
        // A top-level collection has no parent to fall back to — `/my` is the
        // top of the drill-down.
        assert_eq!(
            route_after_delete(&path_of(1), &del(1, None, &[1])).as_deref(),
            Some("/my")
        );
    }

    #[test]
    fn a_cascaded_descendant_is_just_as_dead() {
        // Deleting `Moved`(9) takes `Inside Moved`(10) with it, so a page
        // standing on 10 must leave too — and it goes to 9's parent, since 9 is
        // gone as well.
        let req = del(9, Some(3), &[9, 10]);
        assert_eq!(
            route_after_delete(&path_of(10), &req).as_deref(),
            Some(path_of(3).as_str())
        );
        // …including from a collection subpage.
        assert_eq!(
            route_after_delete(&format!("{}/needs", path_of(10)), &req).as_deref(),
            Some(path_of(3).as_str())
        );
    }

    #[test]
    fn deleting_something_else_leaves_the_page_alone() {
        let req = del(9, Some(3), &[9, 10]);
        // A different collection — including the deleted node's own parent, the
        // page most likely to have the tree row you right-clicked.
        assert_eq!(route_after_delete(&path_of(3), &req), None);
        assert_eq!(route_after_delete(&path_of(1), &req), None);
        // …and every route that is not standing on a collection at all.
        for path in ["/my", "/my/all", "/my/shopping", "/catalog", "/"] {
            assert_eq!(route_after_delete(path, &req), None, "{path}");
        }
        // A malformed id is not a collection we can be standing on.
        assert_eq!(route_after_delete("/my/collections/not-a-uuid", &req), None);
    }

    #[test]
    fn reorder_carries_the_new_parent_when_it_differs() {
        // C has a child X(1); drop D before X → reparent to C AND position.
        let mut rows = top_level();
        rows.push(row(5, Some(3), "X", 1.0));
        let plan = plan_drop(
            &rows,
            &drag(9, None, &[9]),
            Id::from_u128(5),
            DropIntent::Before,
        );
        assert_eq!(plan, Some((Some(Id::from_u128(3)), Some(0.0))));
    }
}
