//! The My-cards sidebar: the read-only collection tree (specs/app-ui.md →
//! "Collection tree"; design/information-architecture.md → My cards mode).
//!
//! The server returns flat rows ([`shared::CollectionTree`]); [`assemble`]
//! rebuilds nesting from `parent_id` and rolls badge counts up through each
//! subtree. Pinned chrome per the IA wireframe: **All cards** (virtual view,
//! not a tree node) above a delimiter, **Inbox** first inside the tree,
//! **Shopping list** below a delimiter at the bottom.
//!
//! Management (drag reparent/reorder, context menus) is the next task; this
//! surface only navigates.

use leptos::prelude::*;
use leptos_router::hooks::use_location;
use shared::{CollectionTreeRow, Id};
use std::collections::{HashMap, HashSet};

use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::collapsible::{Collapsible, CollapsibleContent, CollapsibleTrigger};
use crate::components::ui::item::{Item, ItemSize};
use crate::components::ui::separator::Separator;
use crate::components::ui::skeleton::Skeleton;

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

fn assembled_view(t: AssembledTree, pathname: Memo<String>) -> impl IntoView {
    view! {
        <div class="text-sm">
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
/// link, so collapsing never blocks reaching a collection.
#[component]
fn TreeRow(node: TreeNode, depth: usize, pathname: Memo<String>) -> AnyView {
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

    if children.is_empty() {
        view! {
            <li data-tree-row=id.to_string()>
                <div class="flex items-center" style:padding-left=indent>
                    <span
                        aria-hidden="true"
                        class="text-muted-foreground w-5 shrink-0 text-center text-[10px]"
                    >
                        "•"
                    </span>
                    {link}
                </div>
            </li>
        }
        .into_any()
    } else {
        let toggle_label = format!("Toggle {}", row.summary.name);
        view! {
            <li data-tree-row=id.to_string()>
                <Collapsible default_open=true content_id=format!("tree-children-{id}")>
                    <div class="flex items-center" style:padding-left=indent>
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
                    </div>
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
