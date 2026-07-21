//! Collection-tree management (specs/app-ui.md → Collection tree; the
//! "management" half of the two-task split): one shared context menu over
//! every tree row (create / rename / delete, each confirmed in a dialog)
//! and the commit half of the drag layer (reparent by dropping *onto* a
//! row, reorder by dropping on a row's edge band — fractional `position`
//! midpoints, specs/collection-api.md → Tree CRUD).
//!
//! The client pre-checks cycles only to paint drop targets; the API is the
//! cycle-guard terminus, and its rejections (409 on a cycle, on the Inbox)
//! surface as an error toast rather than being silently swallowed.

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;
use shared::{CollectionKind, CollectionTreeRow, Id};

use super::tree::CollectionTreeResource;
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::context_menu::{ContextMenuContent, ContextMenuItem};
use crate::components::ui::dialog::{
    Dialog, DialogBody, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader,
    DialogTitle,
};
use crate::components::ui::input::Input;
use crate::components::ui::separator::Separator;
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};

/// What the shared context menu is aimed at: one row, or the rail
/// background (top-level create).
#[derive(Clone, PartialEq)]
pub enum MenuTarget {
    Row {
        id: Id,
        name: String,
        is_inbox: bool,
        /// Descendant collections (for the delete confirm).
        descendants: usize,
        /// Rolled-up present copies (for the delete confirm).
        cards: i64,
    },
    Background,
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
    /// Nested collections that cascade with it (for the confirm copy).
    pub descendants: usize,
    /// Rolled-up present copies that cascade with it.
    pub cards: i64,
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
    /// An op in flight (disables dialog submits).
    busy: RwSignal<bool>,
    /// Inline dialog error (server message) — cleared on open/submit.
    error: RwSignal<Option<String>>,
}

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
        busy: RwSignal::new(false),
        error: RwSignal::new(None),
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
                descendants,
                cards,
                ..
            }) => DeleteReq {
                id,
                name,
                descendants,
                cards,
            },
            _ => return,
        };
        self.delete_req.set(Some(subject));
        self.error.set(None);
        self.delete_open.set(true);
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

/// The three management dialogs, mounted once beside the tree.
#[component]
pub fn TreeDialogs() -> impl IntoView {
    let manage = expect_context::<TreeManage>();
    let tree = expect_context::<CollectionTreeResource>().0;

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
        spawn_local(async move {
            match crate::delete_collection(req.id).await {
                Ok(()) => {
                    manage.busy.set(false);
                    manage.delete_open.set(false);
                    tree.refetch();
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
            .map(|r| (r.name, r.descendants, r.cards))
    };

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
    }
}

/// Commit a drop: `Into` reparents; an edge band reorders among the target's
/// siblings (reparenting first when they differ). Sibling positions come from
/// the flat server rows — the render order pins the Inbox first, but
/// `position` math must follow the server's (position, name) order.
pub fn commit_drop(
    tree: CollectionTreeResource,
    toast: ToastHandle,
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

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{CollectionKind, CollectionSummary};

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
