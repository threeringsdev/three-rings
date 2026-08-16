//! The My-cards **root drill-down list** — what `/my` is on a phone
//! (`design/wireframes.pen` → *Mobile — My cards root*, 390×844;
//! `design/information-architecture.md` → "My cards tab is a drill-down: the
//! root screen mirrors the sidebar").
//!
//! Three things are worth knowing before editing this file.
//!
//! **It is the sidebar's top level, not a second data model.** The rows come
//! out of the same [`AssembledTree`] the desktop rail renders — same fetch
//! (shell-level `CollectionTreeResource`), same Inbox pin, same sibling order,
//! same rolled-up badge counts. [`root_rows`] is the whole projection and is a
//! pure function over that tree, so what the list shows and what the rail shows
//! cannot drift.
//!
//! **`Binders` and `Decks` in the wireframe are user collections, not synthetic
//! groups.** The frame's two folder rows read like categories, but the IA sketch
//! they come from (`information-architecture.md` lines 21–34) has them as
//! ordinary top-level binders holding `Trade`/`Bulk` and `Grixis`. So this list
//! groups nothing: it is `t.roots` at depth 0, verbatim. Nested collections are
//! reached by drilling in — the collection view renders its children as folder
//! rows — which is exactly what "drill-down" means.
//!
//! **The breakpoint switch is still pure CSS here.** This list is `md:hidden`
//! and the All-cards table beside it is `hidden md:flex`, exactly as before —
//! the same one-markup-at-every-width rule the rail drawer and the collection
//! view's breadcrumb/back-link pair follow. Nothing in *this* file resolves a
//! media query, so the markup the server sends is the markup that hydrates.
//!
//! What changed under it (P6-166) is on the table's side, not this one: `/my`
//! no longer *SSRs* the table's rows, because a phone paid the aggregate read
//! and downloaded fifty `<tr>`s it never displayed. The table's subtree is
//! mounted after hydration, gated on the same 768 px line its CSS uses — see
//! `super::all_cards::AllCardsPage`. This list is unaffected: it is SSR'd at
//! every width off the shell's own tree resource, which the rail fetches
//! anyway.

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use shared::Id;

use super::tree::{assemble, AssembledTree, CollectionTreeResource};
use crate::components::states::{StateBadge, Tone};
use crate::components::ui::item::{Item, ItemSize};
use crate::components::ui::separator::Separator;
use crate::components::ui::skeleton::Skeleton;

/// Where the `All cards` row drills to: the table, at every width.
pub const ALL_CARDS_PATH: &str = "/my/all";

/// The wireframe's four lucide icons (`layers` / `inbox` / `folder` /
/// `shopping-cart`) as the emoji glyphs this app actually ships — no icon set is
/// vendored, and the desktop tree's pinned rows already use exactly these three
/// for the same three destinations. A collection gets the folder glyph whatever
/// its kind: the frame gives `folder` to both `Binders` and `Decks`, and the
/// file-explorer metaphor makes every collection a folder ("a binder with no
/// cards of its own acts as one").
const ICON_ALL_CARDS: &str = "🗂";
const ICON_INBOX: &str = "📥";
const ICON_COLLECTION: &str = "📁";
const ICON_SHOPPING: &str = "🛒";
/// The sidebar's `PinnedLinkRow` uses the same glyph (`tree.rs`) — one rule
/// for what "recently deleted" reads as everywhere.
const ICON_RECENTLY_DELETED: &str = "🗑";

/// One chevroned row of the root list.
#[derive(Debug, Clone, PartialEq)]
pub struct RootRow {
    pub href: String,
    pub icon: &'static str,
    pub label: String,
    /// The rolled-up count the same row's sidebar badge shows. `None` when it
    /// is not *known* — the tree read failed and [`fallback_rows`] is standing
    /// in — which renders as no count rather than as a `0` the app cannot vouch
    /// for.
    pub count: Option<i64>,
    /// The frame sets `All cards` to weight 600 and every other label to
    /// normal — it is the aggregate, not a sibling of the collections.
    pub strong: bool,
    /// A `$border` rule sits above this row: the frame's two dividers separate
    /// the aggregate from the tree, and the tree from the pinned system row.
    pub divider_before: bool,
    /// The collection this row targets, for rows that target one. `None` on the
    /// two system rows — and the reason they are distinguishable in the DOM.
    pub collection: Option<Id>,
}

/// The list, projected off the assembled tree: `All cards` · divider ·
/// the top-level collections (Inbox pinned first, by [`assemble`]) · divider ·
/// `Shopping list` · `Recently deleted`.
///
/// `all_cards_href` is a parameter rather than a constant so `/my?q=…` survives
/// the trip: the table lives one route down on a phone, and a search deep link
/// that landed on the list should be one tap from its results rather than lost.
///
/// A divider is emitted only when the group under it exists — an empty tree
/// (no rows at all, not even an Inbox) would otherwise draw two rules in a row.
pub fn root_rows(t: &AssembledTree, all_cards_href: &str) -> Vec<RootRow> {
    let mut rows = Vec::with_capacity(t.roots.len() + 2);
    rows.push(all_cards_row(all_cards_href, Some(t.total_present)));
    let mut first_collection = true;
    for node in &t.roots {
        let id = node.row.summary.id;
        rows.push(RootRow {
            href: format!("/my/collections/{id}"),
            icon: if node.row.summary.is_inbox {
                ICON_INBOX
            } else {
                ICON_COLLECTION
            },
            label: node.row.summary.name.clone(),
            count: Some(node.rolled_up),
            strong: false,
            divider_before: std::mem::take(&mut first_collection),
            collection: Some(id),
        });
    }
    rows.push(shopping_row(Some(t.shopping_short)));
    rows.push(recently_deleted_row());
    rows
}

/// The rows that do **not** depend on the collection tree — what the list shows
/// when that read fails.
///
/// This is a way out, not a nicety. Below `md` this list is the *only*
/// navigation `/my` has: the All-cards table beside it is `display: none`, the
/// `☰` rail drawer reads the very same resource and fails with it, and the
/// bottom `My cards` tab links back here. Rendering only an error line therefore
/// turned a partial backend failure into a total, phone-only dead end — no route
/// to the aggregate table, the shopping list, or anything else. Neither
/// destination below needs the tree, so neither has any business disappearing
/// with it.
///
/// Counts are omitted rather than zeroed: both totals come out of the read that
/// just failed, and `0` would be a number the app cannot vouch for.
pub fn fallback_rows(all_cards_href: &str) -> Vec<RootRow> {
    vec![
        all_cards_row(all_cards_href, None),
        shopping_row(None),
        recently_deleted_row(),
    ]
}

fn all_cards_row(href: &str, count: Option<i64>) -> RootRow {
    RootRow {
        href: href.to_string(),
        icon: ICON_ALL_CARDS,
        label: "All cards".into(),
        count,
        strong: true,
        divider_before: false,
        collection: None,
    }
}

fn shopping_row(count: Option<i64>) -> RootRow {
    RootRow {
        href: "/my/shopping".into(),
        icon: ICON_SHOPPING,
        label: "Shopping list".into(),
        count,
        strong: false,
        // Always: there is always at least the aggregate row above it.
        divider_before: true,
        collection: None,
    }
}

/// The "Recently deleted" row (specs/collection-deletion.md → step 5) —
/// this list's mobile counterpart to the sidebar's `PinnedLinkRow`
/// (`tree.rs`), so the page is reachable on a phone without the rail
/// drawer. `count: None` always, not "unknown": the spec is explicit that
/// this list carries **no counts**, which is a different claim from the
/// tree-read-failure meaning `None` carries elsewhere in this file. Joins
/// the `Shopping list` row's own divider group rather than drawing a second
/// rule — the same "pinned system rows" cluster the sidebar keeps them in.
fn recently_deleted_row() -> RootRow {
    RootRow {
        href: "/my/recently-deleted".into(),
        icon: ICON_RECENTLY_DELETED,
        label: "Recently deleted".into(),
        count: None,
        strong: false,
        divider_before: false,
        collection: None,
    }
}

/// `/my` below `md`: the wireframe's screen — title, then the list.
///
/// The frame's header is `My cards` + a 30 px avatar with `space_between`; the
/// avatar is the shell's own top-bar `UserMenu`, one row above, so only the
/// title lives here. Two `<h1>`s therefore exist on `/my` (this one and the
/// table's `All cards`) with exactly one of them ever `display`ed — `display:
/// none` removes the other from the accessibility tree, so a phone is announced
/// one heading and a desktop the other.
#[component]
pub fn MyRootNav() -> impl IntoView {
    let tree = expect_context::<CollectionTreeResource>().0;
    let query = use_query_map();
    // Carry `?q=`/`?cursor=` down to the table: see `root_rows`.
    let all_cards_href = Memo::new(move |_| {
        let q = query.read();
        // The layout carries down too, same as `q`/`cursor`: a grid-mode
        // `/my?view=grid` link opened on a phone should drill into `/my/all`
        // already in grid, not silently reset to the table.
        let list_view =
            !super::all_cards::is_grid_view(q.get(super::all_cards::VIEW_PARAM).as_deref());
        super::all_cards::my_url(
            ALL_CARDS_PATH,
            &q.get("q").unwrap_or_default(),
            list_view,
            q.get("cursor").as_deref(),
        )
    });

    view! {
        <div class="flex min-w-0 flex-col md:hidden" data-testid="my-root">
            <h1 class="px-4 pt-[18px] pb-2.5 text-xl font-semibold">"My cards"</h1>
            <Suspense fallback=list_skeleton>
                {move || {
                    // Read in the tracked render scope, not inside the async
                    // block, where the read lands after the await and outside
                    // this effect's dependency set.
                    let href = all_cards_href.get();
                    Suspend::new(async move {
                        match tree.await {
                            Some(Ok(dto)) => {
                                view! { <MyRootList rows=root_rows(&assemble(dto), &href) /> }
                                    .into_any()
                            }
                            // The tree read failed — name *that*, and still
                            // render the two rows that never needed it. See
                            // `fallback_rows`: without them a phone's My-cards
                            // mode is a dead end, because this list is the only
                            // navigation it has here.
                            Some(Err(e)) => {
                                // The same decision the rail makes about the same
                                // read — one function, so the two cannot disagree
                                // about whether asking again is possible. This
                                // surface offered no retry at all before, which was
                                // the wrong half of that disagreement: the read is
                                // the shell's, a refetch re-renders both, and on a
                                // phone this list is the only navigation `/my` has.
                                let retryable = super::tree::tree_retryable(&e);
                                let failure = crate::components::states::describe(&e).0;
                                view! {
                                    // The `warning` tone is the badge for exactly
                                    // this shape: the list below is real and
                                    // usable, and shorter than it should be. It
                                    // says "some of this is missing" to a reader
                                    // who skims past the sentence — which matters
                                    // most here, because the rows *look* complete.
                                    <div
                                        class="flex flex-col items-start gap-1 px-4 pb-1"
                                        data-failure=failure.slug()
                                    >
                                        <StateBadge tone=Tone::Partial label="Partial" />
                                        <p
                                            role="alert"
                                            data-testid="my-root-error"
                                            class="text-muted-foreground text-sm"
                                        >
                                            "Couldn't load your collections. Everything else here still works."
                                        </p>
                                        {retryable
                                            .then(|| {
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="text-muted-foreground hover:text-foreground text-sm underline"
                                                        data-testid="my-root-retry"
                                                        on:click=move |_| tree.refetch()
                                                    >
                                                        "Try again"
                                                    </button>
                                                }
                                            })}
                                    </div>
                                    <MyRootList rows=fallback_rows(&href) />
                                }
                                    .into_any()
                            }
                            // Anonymous shell — the `/my/*` guard bounces this
                            // load anyway.
                            None => "".into_any(),
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

fn list_skeleton() -> impl IntoView {
    view! {
        <div class="space-y-2 px-2 py-1" aria-busy="true" aria-label="Loading your collections">
            {(0..5).map(|_| view! { <Skeleton class="h-11 w-full" /> }).collect_view()}
        </div>
    }
}

/// The rows themselves — separate from [`MyRootNav`] so the bench can render
/// them over a fixed tree, without the shell's resource or its context.
#[component]
pub fn MyRootList(rows: Vec<RootRow>) -> impl IntoView {
    view! {
        <nav aria-label="My cards" class="flex flex-col px-2 py-1" data-testid="my-root-list">
            {rows.into_iter().map(|row| view! { <RootListRow row /> }).collect_view()}
        </nav>
    }
}

/// One row: icon · label · count · chevron, at the frame's metrics.
///
/// `min-h-11` is the 44 px touch target the frame's `13 px` vertical padding on
/// a `15 px` label adds up to, stated as the requirement rather than
/// reconstructed from paddings.
#[component]
fn RootListRow(row: RootRow) -> impl IntoView {
    let RootRow {
        href,
        icon,
        label,
        count,
        strong,
        divider_before,
        collection,
    } = row;
    let label_class = if strong {
        "min-w-0 flex-1 truncate text-[15px] font-semibold"
    } else {
        "min-w-0 flex-1 truncate text-[15px]"
    };
    view! {
        // The frame's `M Divider Wrap` — a padded wrapper around a
        // fill-container rule, and not a margin on the rule itself: `Separator`
        // is `w-full`, so `mx-2.5` made every divider 20 px wider than its
        // container and gave the page 2 px of horizontal scroll at 390 px.
        {divider_before
            .then(|| {
                view! {
                    <div class="px-2.5 py-1.5">
                        <Separator />
                    </div>
                }
            })}
        <Item
            href=href
            size=ItemSize::Sm
            class="min-h-11 w-full gap-2.5 px-2.5"
            {..}
            data-testid="my-root-row"
            data-collection=collection.map(|id| id.to_string())
        >
            <span aria-hidden="true">{icon}</span>
            <span class=label_class>{label}</span>
            // Omitted, not emptied, when the count is unknown: an empty count
            // cell would still answer `[data-testid=my-root-count]` and let a
            // test read a missing number as a rendered one.
            {count
                .map(|n| {
                    view! {
                        <span
                            class="text-muted-foreground shrink-0 text-[13px] tabular-nums"
                            data-testid="my-root-count"
                        >
                            {n}
                        </span>
                    }
                })}
            <span aria-hidden="true" class="text-muted-foreground shrink-0 text-base leading-none">
                "›"
            </span>
        </Item>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{CollectionKind, CollectionSummary, CollectionTree, CollectionTreeRow};

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
            desired: 0,
        }
    }

    /// The IA sketch's own shape (information-architecture.md lines 21–34):
    /// Inbox(7), Binders(5) > Trade(120) + Bulk(520), Decks(72) > Grixis(100),
    /// two cards short on the shopping list. Server order puts Inbox last, to
    /// prove the pin.
    fn ia_tree() -> AssembledTree {
        assemble(CollectionTree {
            collections: vec![
                row(1, None, "Binders", false, 5),
                row(2, Some(1), "Trade", false, 120),
                row(3, Some(1), "Bulk", false, 520),
                row(4, None, "Decks", false, 72),
                row(5, Some(4), "Grixis", false, 100),
                row(6, None, "Inbox", true, 7),
            ],
            shopping_short: 2,
        })
    }

    #[test]
    fn projects_the_frame_row_for_row() {
        let rows = root_rows(&ia_tree(), ALL_CARDS_PATH);
        let shape: Vec<(&str, &str, Option<i64>, bool, bool)> = rows
            .iter()
            .map(|r| {
                (
                    r.label.as_str(),
                    r.icon,
                    r.count,
                    r.strong,
                    r.divider_before,
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                // All cards is the aggregate: every present copy, Inbox
                // included (7 + 645 + 172), emphasized, no rule above it.
                ("All cards", ICON_ALL_CARDS, Some(824), true, false),
                // A rule, then the tree — Inbox pinned first despite the
                // server having returned it last.
                ("Inbox", ICON_INBOX, Some(7), false, true),
                ("Binders", ICON_COLLECTION, Some(645), false, false),
                ("Decks", ICON_COLLECTION, Some(172), false, false),
                // A second rule, then the two pinned system rows — Recently
                // deleted joins Shopping list's group rather than drawing a
                // third rule (specs/collection-deletion.md → step 5).
                ("Shopping list", ICON_SHOPPING, Some(2), false, true),
                (
                    "Recently deleted",
                    ICON_RECENTLY_DELETED,
                    None,
                    false,
                    false
                ),
            ]
        );
    }

    #[test]
    fn a_failed_tree_read_still_offers_a_way_out() {
        // The escape hatch: below `md` this list is the only navigation `/my`
        // has, so a failed tree read must not take the three tree-independent
        // destinations with it.
        let rows = fallback_rows(ALL_CARDS_PATH);
        assert_eq!(
            rows.iter().map(|r| r.href.as_str()).collect::<Vec<_>>(),
            ["/my/all", "/my/shopping", "/my/recently-deleted"]
        );
        // No counts: both totals come from the read that failed, and a `0`
        // would be a number the app cannot vouch for (Recently deleted never
        // has a count regardless — see `recently_deleted_row`).
        assert!(rows.iter().all(|r| r.count.is_none()));
        // One rule, before Shopping list; none above the first, none before
        // Recently deleted (it joins Shopping list's own group).
        assert!(!rows[0].divider_before);
        assert!(rows[1].divider_before);
        assert!(!rows[2].divider_before);
        // A search that landed on the list still rides down to the table.
        assert_eq!(fallback_rows("/my/all?q=bolt")[0].href, "/my/all?q=bolt");
    }

    #[test]
    fn only_the_top_level_is_listed() {
        // The frame is depth 0: `Trade`, `Bulk` and `Grixis` are reached by
        // drilling into their parents, not by a flattened list.
        let rows = root_rows(&ia_tree(), ALL_CARDS_PATH);
        for hidden in ["Trade", "Bulk", "Grixis"] {
            assert!(
                !rows.iter().any(|r| r.label == hidden),
                "{hidden} is nested and must not surface at the root"
            );
        }
    }

    #[test]
    fn binders_and_decks_are_collections_not_groups() {
        // The load-bearing misreading of this frame: its `Binders` / `Decks`
        // rows are ordinary top-level collections, so each one navigates to a
        // collection route and carries its own id.
        let rows = root_rows(&ia_tree(), ALL_CARDS_PATH);
        let binders = rows.iter().find(|r| r.label == "Binders").unwrap();
        assert_eq!(binders.collection, Some(Id::from_u128(1)));
        assert_eq!(
            binders.href,
            "/my/collections/00000000-0000-0000-0000-000000000001"
        );
        let decks = rows.iter().find(|r| r.label == "Decks").unwrap();
        assert_eq!(decks.collection, Some(Id::from_u128(4)));
    }

    #[test]
    fn system_rows_target_the_three_system_routes() {
        let rows = root_rows(&ia_tree(), ALL_CARDS_PATH);
        assert_eq!(rows[0].href, "/my/all");
        assert_eq!(rows[0].collection, None);
        assert_eq!(rows.last().unwrap().href, "/my/recently-deleted");
        assert_eq!(rows.last().unwrap().collection, None);
        let shopping = rows.iter().find(|r| r.label == "Shopping list").unwrap();
        assert_eq!(shopping.href, "/my/shopping");
        assert_eq!(shopping.collection, None);
    }

    #[test]
    fn a_search_deep_link_rides_down_to_the_table() {
        let rows = root_rows(&ia_tree(), "/my/all?q=bolt");
        assert_eq!(rows[0].href, "/my/all?q=bolt");
        // …and only the aggregate row carries it: a collection row is a
        // different collection's cards, not a filtered view of these.
        assert!(rows[1..].iter().all(|r| !r.href.contains("?q=")));
    }

    #[test]
    fn sibling_order_is_the_trees_order() {
        let t = assemble(CollectionTree {
            collections: vec![
                row(1, None, "Zephyr", false, 1),
                row(2, None, "Alpha", false, 2),
                row(3, None, "Inbox", true, 3),
            ],
            shopping_short: 0,
        });
        let labels: Vec<String> = root_rows(&t, ALL_CARDS_PATH)
            .into_iter()
            .map(|r| r.label)
            .collect();
        // Server order preserved (no re-sort), Inbox lifted to the front.
        assert_eq!(
            labels,
            [
                "All cards",
                "Inbox",
                "Zephyr",
                "Alpha",
                "Shopping list",
                "Recently deleted",
            ]
        );
    }

    #[test]
    fn an_empty_tree_draws_one_rule_not_two() {
        let t = assemble(CollectionTree {
            collections: vec![],
            shopping_short: 0,
        });
        let rows = root_rows(&t, ALL_CARDS_PATH);
        assert_eq!(rows.len(), 3);
        assert!(!rows[0].divider_before);
        // The one remaining rule separates the aggregate from the pinned
        // system rows; the tree's own divider has nothing to introduce.
        assert!(rows[1].divider_before);
        // Recently deleted joins that same group, no rule of its own.
        assert!(!rows[2].divider_before);
    }
}
