//! The My-cards sidebar: the collection tree (specs/app-ui.md →
//! "Collection tree"; design/information-architecture.md → My cards mode).
//!
//! The server returns flat rows ([`shared::CollectionTree`]); [`assemble`]
//! rebuilds nesting from `parent_id` and rolls badge counts up through each
//! subtree. Pinned chrome per the IA wireframe: **All cards** (virtual view,
//! not a tree node) above a delimiter, **Inbox** first inside the tree,
//! **Shopping list** below a delimiter at the bottom.
//!
//! Management (context-menu create/rename/**move**/delete with dialog
//! confirms, drag reparent/reorder) lives in [`super::tree_manage`]; this file
//! wires its handlers onto the rows — and owns the three ways *into* that menu
//! (right-click, the row's `⋯` button, the keyboard's menu key; see
//! [`RowShell`]), since drag alone is mouse-only.

use leptos::prelude::*;
use leptos_router::hooks::use_location;
use shared::{CollectionTreeRow, Id};
use std::collections::{HashMap, HashSet};

use super::tree_manage::{commit_drop, DragState, DropIntent, MenuTarget, TreeManage, TreeMenu};
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::collapsible::{Collapsible, CollapsibleContent, CollapsibleTrigger};
use crate::components::ui::context_menu::{use_context_menu, ContextMenu};
use crate::components::ui::item::{Item, ItemSize};
use crate::components::ui::separator::Separator;
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::sonner::ToastHandle;

/// The one tree fetch per document load, provided at the shell so the desktop
/// rail and the mobile tab badge share it (mirroring `CurrentUserResource`).
/// `None` = anonymous shell — the fetch is skipped rather than 401ing on
/// every public page view.
#[derive(Clone, Copy)]
pub struct CollectionTreeResource(
    pub Resource<Option<Result<shared::CollectionTree, ServerFnError<String>>>>,
);

pub fn provide_collection_tree() {
    let user = expect_context::<crate::shell::CurrentUserResource>().0;
    provide_context(CollectionTreeResource(Resource::new(
        || (),
        move |_| async move {
            match user.await {
                Ok(Some(_)) => Some(crate::collection_tree().await),
                _ => None,
            }
        },
    )));
}

/// One assembled node: its server row, the rolled-up badge count (own present
/// + every descendant's), and its children in sibling order.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    pub row: CollectionTreeRow,
    pub rolled_up: i64,
    pub children: Vec<TreeNode>,
}

/// Everything the sidebar renders, derived client-side from the flat read.
#[derive(Debug, Clone, PartialEq)]
pub struct AssembledTree {
    /// Top-level nodes — Inbox pinned first, then server order (position, name).
    pub roots: Vec<TreeNode>,
    /// The All-cards badge: every present copy, Inbox included.
    pub total_present: i64,
    /// The Inbox badge (rolled up) — also the mobile My-cards tab badge.
    pub inbox_count: i64,
    /// The Shopping-list badge: distinct cards short.
    pub shopping_short: i64,
}

/// Rebuild nesting from `parent_id` and roll up counts. Defensive on shapes
/// the server shouldn't produce: a row whose parent is absent surfaces at top
/// level (rather than vanishing with its subtree), and a parent cycle can't
/// recurse — cycle members are unreachable from any root, so they simply
/// don't render (their copies still count toward `total_present`).
pub fn assemble(dto: shared::CollectionTree) -> AssembledTree {
    let rows = dto.collections;
    let ids: HashSet<Id> = rows.iter().map(|r| r.summary.id).collect();

    let mut kids: HashMap<Id, Vec<usize>> = HashMap::new();
    let mut root_idx: Vec<usize> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        match r.summary.parent_id {
            Some(p) if ids.contains(&p) => kids.entry(p).or_default().push(i),
            _ => root_idx.push(i),
        }
    }

    fn build(i: usize, rows: &[CollectionTreeRow], kids: &HashMap<Id, Vec<usize>>) -> TreeNode {
        let children: Vec<TreeNode> = kids
            .get(&rows[i].summary.id)
            .into_iter()
            .flatten()
            .map(|&c| build(c, rows, kids))
            .collect();
        let rolled_up = rows[i].present + children.iter().map(|c| c.rolled_up).sum::<i64>();
        TreeNode {
            row: rows[i].clone(),
            rolled_up,
            children,
        }
    }

    let mut roots: Vec<TreeNode> = root_idx
        .into_iter()
        .map(|i| build(i, &rows, &kids))
        .collect();
    if let Some(pos) = roots.iter().position(|n| n.row.summary.is_inbox) {
        let inbox = roots.remove(pos);
        roots.insert(0, inbox);
    }

    fn find_inbox(nodes: &[TreeNode]) -> Option<i64> {
        for n in nodes {
            if n.row.summary.is_inbox {
                return Some(n.rolled_up);
            }
            if let Some(v) = find_inbox(&n.children) {
                return Some(v);
            }
        }
        None
    }

    AssembledTree {
        total_present: rows.iter().map(|r| r.present).sum(),
        inbox_count: find_inbox(&roots).unwrap_or(0),
        roots,
        shopping_short: dto.shopping_short,
    }
}

/// The sidebar nav. Renders nothing on an anonymous shell (the `/my/*` guard
/// bounces those loads anyway) and a quiet error line if the read fails.
#[component]
pub fn CollectionTreeNav() -> impl IntoView {
    let tree = expect_context::<CollectionTreeResource>().0;
    let pathname = use_location().pathname;
    // NB: `TreeManage` is provided by the app shell, not here — ⌘K's
    // `New binder…` opens this same create dialog from Catalog mode, where this
    // component isn't mounted. `TreeDialogs` is mounted by the shell for the
    // same family of reason plus one more: this nav lives in an `aside` that is
    // off-screen below `md`, and a dialog cannot be shown from inside a hidden
    // subtree — mounting the dialogs here made every tree action invisible on a
    // phone.

    view! {
        <nav aria-label="Collections">
            <Suspense fallback=tree_skeleton>
                {move || Suspend::new(async move {
                    match tree.await {
                        Some(Ok(dto)) => assembled_view(assemble(dto), pathname).into_any(),
                        Some(Err(_)) => {
                            view! {
                                <p class="text-muted-foreground px-2 text-xs">
                                    "Couldn't load collections."
                                </p>
                            }
                                .into_any()
                        }
                        None => "".into_any(),
                    }
                })}
            </Suspense>
        </nav>
    }
}

fn tree_skeleton() -> impl IntoView {
    view! {
        <div class="space-y-2">
            <Skeleton class="h-5 w-3/4" />
            <Skeleton class="h-5 w-1/2" />
            <Skeleton class="h-5 w-2/3" />
            <Skeleton class="h-5 w-1/2" />
        </div>
    }
}

/// The assembled tree, wrapped in one shared [`ContextMenu`]. The wrapper
/// lives here — inside the `Suspend` — rather than in `CollectionTreeNav`
/// so its `Provider` and the rows share one synchronous owner: a `Provider`
/// placed above the `Suspense` boundary does not reach `use_context_menu()`
/// calls made inside the resolved async view (whereas `TreeManage`, provided
/// in the component body, does). `TreeBody` reads the menu handle from *under*
/// the wrapper.
fn assembled_view(t: AssembledTree, pathname: Memo<String>) -> impl IntoView {
    view! {
        <ContextMenu id="tree">
            <TreeBody t pathname />
            <TreeMenu />
        </ContextMenu>
    }
}

#[component]
fn TreeBody(t: AssembledTree, pathname: Memo<String>) -> impl IntoView {
    let manage = expect_context::<TreeManage>();
    let menu = use_context_menu();

    view! {
        // Right-click on the rail background (not a row — rows stop
        // propagation) → top-level create menu.
        <div
            class="text-sm"
            data-tree-root
            on:contextmenu=move |ev| {
                ev.prevent_default();
                manage.menu_target.set(Some(MenuTarget::Background));
                if let Some(menu) = menu {
                    menu.open_at(f64::from(ev.client_x()), f64::from(ev.client_y()));
                }
            }
        >
            <PinnedRow
                href="/my"
                icon="🗂"
                label="All cards"
                count=t.total_present
                pathname
            />
            <Separator class="my-2" />
            <ul class="space-y-0.5">
                {t
                    .roots
                    .into_iter()
                    .map(|node| view! { <TreeRow node depth=0 pathname /> })
                    .collect_view()}
            </ul>
            <Separator class="my-2" />
            <PinnedRow
                href="/my/shopping"
                icon="🛒"
                label="Shopping list"
                count=t.shopping_short
                pathname
            />
        </div>
    }
}

/// A pinned system row (All cards / Shopping list) — an `Item` link with the
/// wireframe's icon and a count badge.
#[component]
fn PinnedRow(
    href: &'static str,
    icon: &'static str,
    label: &'static str,
    count: i64,
    pathname: Memo<String>,
) -> impl IntoView {
    view! {
        <Item
            href=href
            size=ItemSize::Xs
            class="aria-[current=page]:bg-accent aria-[current=page]:text-accent-foreground w-full"
            {..}
            aria-current=move || (pathname.get() == href).then_some("page")
        >
            <span aria-hidden="true">{icon}</span>
            <span class="min-w-0 flex-1 truncate font-medium">{label}</span>
            <Badge variant=BadgeVariant::Muted size=BadgeSize::Sm class="shrink-0">
                {count}
            </Badge>
        </Item>
    }
}

/// One tree row (recursive). Parents wrap their children in a `Collapsible`
/// whose chevron is the trigger; the name itself is always the navigation
/// link, so collapsing never blocks reaching a collection. [`RowShell`]
/// carries the management wiring (context menu, drag) for both shapes.
#[component]
fn TreeRow(node: TreeNode, depth: usize, pathname: Memo<String>) -> AnyView {
    // Self + every descendant — the drop targets this row may not take
    // (client-side cycle pre-check) and the delete-confirm's subtree count.
    let mut forbidden = HashSet::new();
    subtree_ids(&node, &mut forbidden);

    let TreeNode {
        row,
        rolled_up,
        children,
    } = node;
    let id = row.summary.id;
    let indent = format!("{}rem", depth as f64 * 0.75);
    let link = row_link(
        format!("/my/collections/{id}"),
        row.summary.name.clone(),
        row.summary.is_inbox.then_some("📥"),
        rolled_up,
        pathname,
    );
    let name = row.summary.name.clone();
    let is_inbox = row.summary.is_inbox;
    let parent_id = row.summary.parent_id;

    if children.is_empty() {
        view! {
            <li data-tree-row=id.to_string()>
                <RowShell id name is_inbox parent_id forbidden cards=rolled_up indent>
                    <span
                        aria-hidden="true"
                        class="text-muted-foreground w-5 shrink-0 text-center text-[10px]"
                    >
                        "•"
                    </span>
                    {link}
                </RowShell>
            </li>
        }
        .into_any()
    } else {
        let toggle_label = format!("Toggle {}", row.summary.name);
        view! {
            <li data-tree-row=id.to_string()>
                <Collapsible default_open=true content_id=format!("tree-children-{id}")>
                    <RowShell id name is_inbox parent_id forbidden cards=rolled_up indent>
                        <CollapsibleTrigger
                            class="text-muted-foreground hover:text-foreground w-5 shrink-0 rounded-sm text-center"
                            attr:aria-label=toggle_label
                        >
                            <span
                                aria-hidden="true"
                                class="inline-block transition-transform duration-200 [[data-state=open]>&]:rotate-90"
                            >
                                "›"
                            </span>
                        </CollapsibleTrigger>
                        {link}
                    </RowShell>
                    <CollapsibleContent>
                        <ul class="space-y-0.5">
                            {children
                                .into_iter()
                                .map(|node| view! { <TreeRow node depth={depth + 1} pathname /> })
                                .collect_view()}
                        </ul>
                    </CollapsibleContent>
                </Collapsible>
            </li>
        }
        .into_any()
    }
}

/// The row container: indentation plus the management wiring shared by leaf
/// and parent rows — right-click (or the row's `⋯` button, or the keyboard's
/// menu key) aims the shared context menu, and the drag handlers implement
/// reparent (drop into) / reorder (drop on an edge band) with the drop hint
/// painted via `data-drop-hint`.
///
/// **Three ways in, because right-click is only one of them.** Right-click is
/// the desktop-mouse gesture; the `⋯` button is the tab-reachable and
/// tap-reachable one (a real `<button>`, revealed on hover at `md` and up so
/// the at-rest row still reads as the wireframe's `Tree Item`, always shown
/// below it where there is no hover); and `ContextMenu`/`Shift+F10` from
/// anywhere in the row is the platform keyboard contract for opening a context
/// menu. All three set the same `menu_target` and open the same shared panel —
/// the only difference is where it is anchored.
///
/// The `⋯` button is not a convenience. A held touch does **not** produce a
/// `contextmenu` on the Android webview — measured, against a comment in this
/// repo that had claimed otherwise since the menu was vendored
/// (`end2end/android-tree-move-check.mjs`) — so without an explicit trigger a
/// phone has no way into this menu at all.
#[component]
fn RowShell(
    id: Id,
    name: String,
    is_inbox: bool,
    parent_id: Option<Id>,
    forbidden: HashSet<Id>,
    cards: i64,
    indent: String,
    children: Children,
) -> impl IntoView {
    let manage = expect_context::<TreeManage>();
    let menu = use_context_menu();
    let toast = expect_context::<ToastHandle>();
    let tree = expect_context::<CollectionTreeResource>();

    // Shared by three handlers, so stored once rather than cloned per closure.
    let row_name = StoredValue::new(name);
    let row_forbidden = StoredValue::new(forbidden.clone());
    let aim = move || {
        manage.menu_target.set(Some(MenuTarget::Row {
            id,
            name: row_name.get_value(),
            is_inbox,
            parent_id,
            forbidden: row_forbidden.get_value(),
            cards,
        }));
    };

    let hint = move || {
        manage
            .drop_hint
            .get()
            .filter(|(h, _)| *h == id)
            .map(|(_, i)| i.as_str())
    };

    view! {
        // `data-tree-row-head` marks *this* row's own clickable/draggable
        // strip — distinct from a parent `<li>`'s `Collapsible` wrapper, which
        // also contains descendant heads. Tests and drag both target the head.
        <div
            data-tree-row-head=id.to_string()
            class="group/row data-[drop-hint=into]:bg-accent/60 data-[drop-hint=into]:rounded-md data-[drop-hint=before]:shadow-[inset_0_2px_0_0_var(--color-ring)] data-[drop-hint=after]:shadow-[inset_0_-2px_0_0_var(--color-ring)] flex items-center"
            style:padding-left=indent
            draggable=(!is_inbox).then_some("true")
            data-drop-hint=hint
            on:contextmenu=move |ev| {
                ev.prevent_default();
                ev.stop_propagation();
                aim();
                if let Some(menu) = menu {
                    menu.open_at(f64::from(ev.client_x()), f64::from(ev.client_y()));
                }
            }
            on:keydown=move |ev| {
                // The platform's own keyboard route to a context menu. Browsers
                // *do* synthesize `contextmenu` from these, but at coordinates
                // that are 0,0 on a keyboard activation — handling the keys
                // ourselves (and `prevent_default`ing the synthesized event
                // away) is what lets the panel be anchored to the row instead of
                // the viewport corner.
                let key = ev.key();
                if key != "ContextMenu" && !(key == "F10" && ev.shift_key()) {
                    return;
                }
                ev.prevent_default();
                ev.stop_propagation();
                aim();
                if let Some(menu) = menu {
                    let (x, y) = element_anchor(ev.as_ref()).unwrap_or((0.0, 0.0));
                    menu.open_at(x, y);
                }
            }
            on:dragstart=move |ev| {
                if is_inbox {
                    // The Inbox is pinned and unreparentable — cancel the
                    // native link drag its `<a>` would otherwise start.
                    ev.prevent_default();
                    return;
                }
                begin_drag(&ev, id);
                manage
                    .drag
                    .set(
                        Some(DragState {
                            id,
                            parent_id,
                            forbidden: forbidden.clone(),
                        }),
                    );
            }
            on:dragover=move |ev| {
                let Some(drag) = manage.drag.get_untracked() else {
                    return;
                };
                if drag.forbidden.contains(&id) {
                    return;
                }
                let Some(intent) = drop_intent(&ev, is_inbox) else {
                    return;
                };
                ev.prevent_default();
                if manage.drop_hint.get_untracked() != Some((id, intent)) {
                    manage.drop_hint.set(Some((id, intent)));
                }
            }
            on:drop=move |ev| {
                ev.prevent_default();
                manage.drop_hint.set(None);
                let Some(drag) = manage.drag.get_untracked() else {
                    return;
                };
                manage.drag.set(None);
                if drag.forbidden.contains(&id) {
                    return;
                }
                let Some(intent) = drop_intent(&ev, is_inbox) else {
                    return;
                };
                commit_drop(tree, toast, drag, id, intent);
            }
            on:dragend=move |_| {
                manage.drag.set(None);
                manage.drop_hint.set(None);
            }
        >
            {children()}
            // The mouse-free trigger. `opacity-0` and not `hidden` at `md` and
            // up: a hidden button is not tab-reachable, and being reachable by
            // Tab is the entire point of it — `focus-visible` brings it back
            // into view for whoever got here without a pointer.
            <button
                type="button"
                data-tree-row-actions=id.to_string()
                aria-haspopup="menu"
                aria-label=format!("Actions for {}", row_name.get_value())
                class="text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-ring mr-1 w-5 shrink-0 rounded-sm text-center text-sm leading-5 focus-visible:ring-1 focus-visible:outline-none md:opacity-0 md:group-hover/row:opacity-100 md:focus-visible:opacity-100"
                on:click=move |ev| {
                    // The row is a link; a click on its actions button is not a
                    // navigation.
                    ev.prevent_default();
                    ev.stop_propagation();
                    aim();
                    if let Some(menu) = menu {
                        // The button's own rect, never `client_x/y`: a keyboard
                        // activation (⏎/space on the focused button) fires a
                        // click whose coordinates are 0,0, which would park the
                        // panel in the viewport corner.
                        let (x, y) = element_anchor(ev.as_ref()).unwrap_or((0.0, 0.0));
                        menu.open_at(x, y);
                    }
                }
            >
                <span aria-hidden="true">"⋯"</span>
            </button>
        </div>
    }
}

/// Viewport coordinates to anchor the shared context menu at: the bottom-left
/// of whatever element the handler is bound to. Hydrate-only — the measurement
/// API exists client-side, and so does every gesture that calls this.
#[cfg(feature = "hydrate")]
fn element_anchor(ev: &leptos::web_sys::Event) -> Option<(f64, f64)> {
    use leptos::wasm_bindgen::JsCast;
    let el = ev
        .current_target()?
        .dyn_into::<leptos::web_sys::HtmlElement>()
        .ok()?;
    let rect = el.get_bounding_client_rect();
    Some((rect.left(), rect.bottom()))
}

#[cfg(not(feature = "hydrate"))]
fn element_anchor(_ev: &leptos::web_sys::Event) -> Option<(f64, f64)> {
    None
}

/// Collect a node's id plus every descendant's.
fn subtree_ids(node: &TreeNode, out: &mut HashSet<Id>) {
    out.insert(node.row.summary.id);
    for c in &node.children {
        subtree_ids(c, out);
    }
}

/// Mark the drag for the browser: Firefox won't start a drag without
/// `setData`, and "move" is the honest effect. Hydrate-only (the
/// `DataTransfer` APIs aren't in the SSR build's web-sys feature set).
#[cfg(feature = "hydrate")]
fn begin_drag(ev: &leptos::web_sys::DragEvent, id: Id) {
    if let Some(dt) = ev.data_transfer() {
        dt.set_effect_allowed("move");
        let _ = dt.set_data("text/plain", &id.to_string());
    }
}

#[cfg(not(feature = "hydrate"))]
fn begin_drag(_ev: &leptos::web_sys::DragEvent, _id: Id) {}

/// Where on the row this drag sits: the top/bottom quarter bands mean
/// reorder (before/after among siblings), the middle means reparent into.
/// The Inbox only ever accepts `Into` — it is pinned first client-side, so
/// ordering relative to it is meaningless. Hydrate-only (needs rect math).
#[cfg(feature = "hydrate")]
fn drop_intent(ev: &leptos::web_sys::DragEvent, is_inbox: bool) -> Option<DropIntent> {
    use leptos::wasm_bindgen::JsCast;
    let el = ev
        .current_target()?
        .dyn_into::<leptos::web_sys::HtmlElement>()
        .ok()?;
    let rect = el.get_bounding_client_rect();
    let y = f64::from(ev.client_y()) - rect.top();
    let h = rect.height().max(1.0);
    Some(if is_inbox {
        DropIntent::Into
    } else if y < h * 0.25 {
        DropIntent::Before
    } else if y > h * 0.75 {
        DropIntent::After
    } else {
        DropIntent::Into
    })
}

#[cfg(not(feature = "hydrate"))]
fn drop_intent(_ev: &leptos::web_sys::DragEvent, _is_inbox: bool) -> Option<DropIntent> {
    None
}

/// The navigation link + count badge shared by leaf and parent rows.
fn row_link(
    href: String,
    name: String,
    icon: Option<&'static str>,
    count: i64,
    pathname: Memo<String>,
) -> impl IntoView {
    // Prefix match, not equality: a collection stays selected on its own
    // subpages (`/my/collections/{id}/needs`) — you are still *in* that
    // collection. The pinned rows keep exact matching (`/my` is a prefix of
    // every collection route). Codex review, this task.
    let current = {
        let href = href.clone();
        move || {
            let p = pathname.get();
            (p == href || p.starts_with(&format!("{href}/"))).then_some("page")
        }
    };
    view! {
        <a
            href=href
            class="hover:bg-accent/50 aria-[current=page]:bg-accent aria-[current=page]:text-accent-foreground flex min-w-0 flex-1 items-center gap-2 rounded-md px-2 py-1 transition-colors"
            aria-current=current
        >
            {icon.map(|i| view! { <span aria-hidden="true">{i}</span> })}
            <span class="truncate">{name}</span>
            <Badge variant=BadgeVariant::Muted size=BadgeSize::Sm class="ml-auto shrink-0">
                {count}
            </Badge>
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{CollectionKind, CollectionSummary, CollectionTree};

    fn row(
        id: u128,
        parent: Option<u128>,
        name: &str,
        is_inbox: bool,
        present: i64,
    ) -> CollectionTreeRow {
        CollectionTreeRow {
            summary: CollectionSummary {
                id: Id::from_u128(id),
                parent_id: parent.map(Id::from_u128),
                kind: CollectionKind::Binder,
                name: name.into(),
                is_inbox,
                position: 0.0,
                format: None,
            },
            present,
        }
    }

    fn tree(collections: Vec<CollectionTreeRow>, shopping_short: i64) -> CollectionTree {
        CollectionTree {
            collections,
            shopping_short,
        }
    }

    #[test]
    fn nests_and_rolls_up() {
        // Binders(5) > Trade(120), Bulk(520); Inbox(7) — the IA sample shape.
        let t = assemble(tree(
            vec![
                row(1, None, "Binders", false, 5),
                row(2, Some(1), "Trade", false, 120),
                row(3, Some(1), "Bulk", false, 520),
                row(4, None, "Inbox", true, 7),
            ],
            2,
        ));
        assert_eq!(t.total_present, 652);
        assert_eq!(t.inbox_count, 7);
        assert_eq!(t.shopping_short, 2);
        // Inbox pinned first even though the server sorted it last.
        assert!(t.roots[0].row.summary.is_inbox);
        let binders = &t.roots[1];
        assert_eq!(binders.rolled_up, 645);
        assert_eq!(
            binders
                .children
                .iter()
                .map(|c| &c.row.summary.name)
                .collect::<Vec<_>>(),
            ["Trade", "Bulk"]
        );
    }

    #[test]
    fn sibling_order_is_preserved() {
        let t = assemble(tree(
            vec![
                row(4, None, "Inbox", true, 0),
                row(1, None, "A", false, 0),
                row(2, None, "B", false, 0),
                row(3, Some(2), "B1", false, 0),
            ],
            0,
        ));
        let names: Vec<_> = t
            .roots
            .iter()
            .map(|n| n.row.summary.name.as_str())
            .collect();
        assert_eq!(names, ["Inbox", "A", "B"]);
    }

    #[test]
    fn orphan_surfaces_at_top_level() {
        // Parent 9 was never returned — the row must not vanish.
        let t = assemble(tree(
            vec![
                row(4, None, "Inbox", true, 1),
                row(2, Some(9), "Lost", false, 3),
            ],
            0,
        ));
        assert_eq!(t.roots.len(), 2);
        assert_eq!(t.total_present, 4);
    }

    #[test]
    fn cycle_neither_renders_nor_hangs() {
        let t = assemble(tree(
            vec![
                row(4, None, "Inbox", true, 1),
                row(1, Some(2), "A", false, 10),
                row(2, Some(1), "B", false, 20),
            ],
            0,
        ));
        // Cycle members are unreachable from the roots…
        assert_eq!(t.roots.len(), 1);
        // …but their copies still count toward the All-cards total.
        assert_eq!(t.total_present, 31);
    }

    #[test]
    fn empty_inbox_only() {
        let t = assemble(tree(vec![row(4, None, "Inbox", true, 0)], 0));
        assert_eq!(t.total_present, 0);
        assert_eq!(t.inbox_count, 0);
        assert_eq!(t.roots.len(), 1);
        assert!(t.roots[0].children.is_empty());
    }
}
