//! `/my/shopping` — the global shopping list (specs/app-ui.md → `/my/shopping`;
//! specs/collection-api.md → `ShoppingList`).
//!
//! One row per card nobody holds enough of: the shortfall, which collections
//! want it, and the export.
//!
//! **The export is the deliverable, not a convenience.** collection-api settled
//! CSV import/export as *later* and named this the one export v1 ships, so it is
//! the plain, universal form every store's mass-entry box and every deck editor
//! accepts — `N Card Name`, one line per card, nothing else. No header, no
//! per-collection annotation: a comment syntax that half the paste targets treat
//! as a card name is worse than no comment. [`export_text`] is a pure function
//! with the list as its only input, so the text and the table cannot describe
//! different lists.
//!
//! **The shortfall is global and board-blind, and here that is simply right.**
//! `shopping_list` sums desires across every collection and subtracts everything
//! owned, per oracle card. A deck wanting one copy on its mainboard and one on
//! its sideboard needs two physical cards, and boards are exactly what you stop
//! caring about at the shop counter. (The board-blindness that *is* worth
//! knowing about lives one page over — see the [needs](super::needs) module doc.)

use leptos::prelude::*;
use shared::{ShoppingList, ShoppingRow};

use crate::components::states::{ErrorNote, StateBadge, Tone};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

/// The id of the export box — deterministic and caller-supplied, the convention
/// every interactive piece here follows, and what the Copy button reaches for.
const EXPORT_ID: &str = "shopping-export";

/// The list as pasteable text: `N Card Name` per line, in the list's own order
/// (card name, from the read model).
///
/// Rows with nothing to buy are dropped rather than emitted as `0 Foo` — a
/// pasted zero is a line every target parses differently, and the list should
/// not contain a line the table does not show.
pub fn export_text(rows: &[ShoppingRow]) -> String {
    rows.iter()
        .filter(|r| r.shortfall > 0)
        .map(|r| format!("{} {}", r.shortfall, r.name))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `N cards · M copies` — the two numbers a shopping list is actually judged
/// by, and they are not the same number (a playset of one card is 1 card, 4
/// copies).
fn summary(rows: &[ShoppingRow]) -> String {
    let cards = rows.len();
    let copies: i32 = rows.iter().map(|r| r.shortfall).sum();
    let cards_label = if cards == 1 {
        "1 card".to_string()
    } else {
        format!("{cards} cards")
    };
    let copies_label = if copies == 1 {
        "1 copy".to_string()
    } else {
        format!("{copies} copies")
    };
    format!("{cards_label} · {copies_label}")
}

#[component]
pub fn ShoppingPage() -> impl IntoView {
    // The revision every move bumps: buying is not the only way a shortfall
    // changes — pulling copies into a collection does not, but adding or
    // removing copies anywhere does, and this page is downstream of all of it.
    let revision = super::move_selection::holdings_revision();
    let list = Resource::new(
        move || revision.get(),
        |_revision| async move { crate::shopping_list_view().await },
    );

    view! {
        <div class="flex min-w-0 flex-col gap-4 p-4 md:p-6" data-testid="shopping-page">
            <div>
                <h1 class="text-2xl font-bold">"Shopping list"</h1>
                <p class="text-muted-foreground text-sm">
                    "Cards your collections want that nobody holds enough of."
                </p>
            </div>
            <Transition fallback=|| {
                view! {
                    <div class="space-y-2" aria-busy="true" aria-label="Loading the shopping list">
                        {(0..6).map(|_| view! { <Skeleton class="h-10 w-full" /> }).collect_view()}
                    </div>
                }
            }>
                {move || Suspend::new(async move {
                    match list.await {
                        Ok(list) if list.rows.is_empty() => {
                            view! {
                                // `success`, like the needs page's own empty arm
                                // and for the same reason: an empty shopping list
                                // is the good kind of nothing (every wanted copy
                                // is owned *somewhere*), not the absent kind. The
                                // sentence says which, and the tone says it
                                // without being read.
                                <div
                                    class="text-muted-foreground flex flex-col items-center gap-2 py-12 text-center text-sm"
                                    data-testid="shopping-empty"
                                >
                                    <StateBadge tone=Tone::Resolved label="All set" />
                                    <p>
                                        "Nothing to buy — every card your collections want is already owned."
                                    </p>
                                </div>
                            }
                                .into_any()
                        }
                        Ok(list) => view! { <ShoppingBody list /> }.into_any(),
                        Err(e) => {
                            view! {
                                <ErrorNote
                                    what="Couldn't load the shopping list"
                                    e
                                    testid="shopping-error"
                                    retry=Callback::new(move |()| list.refetch())
                                />
                            }
                                .into_any()
                        }
                    }
                })}
            </Transition>
        </div>
    }
}

#[component]
fn ShoppingBody(list: ShoppingList) -> impl IntoView {
    let rows = list.rows;
    let text = export_text(&rows);
    let head = summary(&rows);

    view! {
        <p class="text-sm font-medium" data-testid="shopping-summary">
            {head}
        </p>
        <TableWrapper class="max-h-none">
            <Table {..} data-testid="shopping-table">
                <TableHeader>
                    <TableRow>
                        <TableHead>"Card"</TableHead>
                        <TableHead>"Wanted by"</TableHead>
                        <TableHead class="text-right">"Want"</TableHead>
                        <TableHead class="text-right">"Own"</TableHead>
                        <TableHead class="text-right">"Buy"</TableHead>
                    </TableRow>
                </TableHeader>
                <TableBody>
                    {rows
                        .into_iter()
                        .map(|row| {
                            let oracle_id = row.oracle_id;
                            view! {
                                <TableRow {..} data-testid="shopping-row" data-oracle=oracle_id.to_string()>
                                    <TableCell class="p-2 font-medium">
                                        <a href=format!("/cards/{oracle_id}") class="hover:underline">
                                            {row.name}
                                        </a>
                                    </TableCell>
                                    <TableCell
                                        class="text-muted-foreground p-2 text-sm"
                                        {..}
                                        data-testid="wanted-by"
                                    >
                                        {row.wanted_by.join(", ")}
                                    </TableCell>
                                    <TableCell class="p-2 text-right tabular-nums">
                                        {row.desired_total}
                                    </TableCell>
                                    <TableCell class="p-2 text-right tabular-nums">
                                        {row.owned}
                                    </TableCell>
                                    <TableCell
                                        class="p-2 text-right font-medium tabular-nums"
                                        {..}
                                        data-testid="shortfall"
                                    >
                                        {row.shortfall}
                                    </TableCell>
                                </TableRow>
                            }
                        })
                        .collect_view()}
                </TableBody>
            </Table>
        </TableWrapper>
        <Export text />
    }
}

/// The text export: the list itself, selectable, plus one-tap copy.
///
/// A read-only `<textarea>` rather than a download or a clipboard-only button:
/// it is SSR'd markup, so the export exists on a page that never hydrates (and
/// in `curl`), it can be selected and copied by hand in any webview, and it is
/// the one shape that needs no permission prompt on mobile.
#[component]
fn Export(text: String) -> impl IntoView {
    let lines = text.lines().count().max(1);
    let copied = RwSignal::new(false);
    let copy = move |_| {
        copied.set(copy_export());
    };

    view! {
        <section class="flex flex-col gap-2" data-testid="shopping-export-section">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <h2 class="text-lg font-semibold">"Export"</h2>
                <div class="flex items-center gap-2">
                    <Show when=move || copied.get()>
                        <span class="text-muted-foreground text-xs" data-testid="export-copied">
                            "Copied"
                        </span>
                    </Show>
                    <Button
                        variant=ButtonVariant::Outline
                        attr:data-testid="export-copy"
                        on:click=copy
                    >
                        "Copy"
                    </Button>
                </div>
            </div>
            <textarea
                id=EXPORT_ID
                readonly
                data-testid="shopping-export"
                aria-label="Shopping list as text"
                class="border-input bg-muted/40 min-h-24 w-full rounded-md border p-2 font-mono text-xs"
                rows=lines.min(20).to_string()
            >
                {text}
            </textarea>
        </section>
    }
}

/// Select the export box and copy it. `execCommand` rather than
/// `navigator.clipboard`: the async clipboard API needs a secure context and a
/// permission the Android WebView does not always grant, while this works in
/// every webview this app ships in and degrades to "the text is still selected,
/// copy it yourself".
fn copy_export() -> bool {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        let doc = leptos::tachys::dom::document();
        let Some(area) = doc
            .get_element_by_id(EXPORT_ID)
            .and_then(|el| el.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
        else {
            return false;
        };
        area.select();
        // `execCommand` hangs off `HtmlDocument`, not `Document`.
        doc.dyn_ref::<web_sys::HtmlDocument>()
            .and_then(|d| d.exec_command("copy").ok())
            .unwrap_or(false)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn row(name: &str, shortfall: i32, wanted_by: &[&str]) -> ShoppingRow {
        ShoppingRow {
            oracle_id: Uuid::new_v4(),
            name: name.to_string(),
            desired_total: shortfall + 1,
            owned: 1,
            shortfall,
            wanted_by: wanted_by.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn the_export_is_one_pasteable_line_per_card() {
        let rows = vec![
            row("Lightning Bolt", 3, &["Commander Deck"]),
            row("Snapcaster Mage", 1, &["Commander Deck", "Cube"]),
        ];
        assert_eq!(export_text(&rows), "3 Lightning Bolt\n1 Snapcaster Mage");
    }

    #[test]
    fn the_export_drops_rows_with_nothing_to_buy() {
        // `shopping_list` filters these out server-side, so this is belt and
        // braces — but a `0 Foo` line is a line every paste target reads
        // differently, and the export must not contain a row the table doesn't.
        let rows = vec![row("Lightning Bolt", 0, &[]), row("Brainstorm", 2, &[])];
        assert_eq!(export_text(&rows), "2 Brainstorm");
        assert_eq!(export_text(&[]), "");
    }

    #[test]
    fn the_summary_counts_cards_and_copies_separately() {
        // A playset of one card is one card and four copies; a summary that
        // conflated them would understate every shopping trip.
        assert_eq!(
            summary(&[row("Lightning Bolt", 4, &[])]),
            "1 card · 4 copies"
        );
        assert_eq!(
            summary(&[row("Lightning Bolt", 1, &[]), row("Brainstorm", 1, &[])]),
            "2 cards · 2 copies"
        );
        assert_eq!(summary(&[]), "0 cards · 0 copies");
    }
}
