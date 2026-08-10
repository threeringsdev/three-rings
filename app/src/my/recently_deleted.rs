//! `/my/recently-deleted` — the "Recently deleted" list
//! (specs/collection-deletion.md → step 5).
//!
//! **Deliberately the weaker recovery path.** The delete toast's Undo (wired
//! in `tree_manage.rs`) reverses a delete *whole*, from its own receipt — the
//! misclick path. This page is for later: no receipt, no counts, and Restore
//! does not promise to put cards or nested collections back where they were —
//! only the collection itself comes back, wherever its contents now sit. The
//! copy on this page must not claim more than that (spec: "not a time
//! machine").
//!
//! No purge, no permanent delete — out of scope per the spec, and this page
//! has no control for either.

use leptos::prelude::*;
use leptos::task::spawn_local;
use shared::{CollectionKind, DeletedCollectionRow};

use crate::components::states::ErrorNote;
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::sonner::{ToastHandle, ToastKind, ToastOptions};
use crate::components::ui::table::{
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};
use crate::my::tree::CollectionTreeResource;

/// "Binder" / "Deck" — the same plain label `collection.rs`'s header uses,
/// not an icon: this list has no tree context to make an icon legible in.
fn kind_label(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Binder => "Binder",
        CollectionKind::Deck => "Deck",
    }
}

#[component]
pub fn RecentlyDeletedPage() -> impl IntoView {
    // Bumped after a successful Restore so the list drops the row — the read
    // is the source of truth (a restored collection stops being
    // `deleted_at IS NOT NULL`), not a client-side splice.
    let revision = RwSignal::new(0u32);
    let list = Resource::new(
        move || revision.get(),
        |_revision| async move { crate::recently_deleted().await },
    );

    view! {
        <div class="flex min-w-0 flex-col gap-4 p-4 md:p-6" data-testid="recently-deleted-page">
            <div>
                <h1 class="text-2xl font-bold">"Recently deleted"</h1>
                <p class="text-muted-foreground text-sm">
                    "Collections you've deleted. Restore brings the collection back — its cards and any nested collections stay wherever they ended up since."
                </p>
            </div>
            <Transition fallback=|| {
                view! {
                    <div
                        class="space-y-2"
                        aria-busy="true"
                        aria-label="Loading recently deleted collections"
                    >
                        {(0..4).map(|_| view! { <Skeleton class="h-10 w-full" /> }).collect_view()}
                    </div>
                }
            }>
                {move || Suspend::new(async move {
                    match list.await {
                        Ok(rows) if rows.is_empty() => {
                            view! {
                                <p
                                    class="text-muted-foreground py-12 text-center text-sm"
                                    data-testid="recently-deleted-empty"
                                >
                                    "Nothing deleted recently."
                                </p>
                            }
                                .into_any()
                        }
                        Ok(rows) => view! { <RecentlyDeletedBody rows revision /> }.into_any(),
                        Err(e) => {
                            view! {
                                <ErrorNote
                                    what="Couldn't load recently deleted collections"
                                    e
                                    testid="recently-deleted-error"
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
fn RecentlyDeletedBody(rows: Vec<DeletedCollectionRow>, revision: RwSignal<u32>) -> impl IntoView {
    view! {
        <TableWrapper class="max-h-none">
            <Table {..} data-testid="recently-deleted-table">
                <TableHeader>
                    <TableRow>
                        <TableHead>"Name"</TableHead>
                        <TableHead>"Kind"</TableHead>
                        <TableHead>"Deleted"</TableHead>
                        <TableHead>
                            <span class="sr-only">"Restore"</span>
                        </TableHead>
                    </TableRow>
                </TableHeader>
                <TableBody>
                    {rows
                        .into_iter()
                        .map(|row| view! { <RecentlyDeletedRow row revision /> })
                        .collect_view()}
                </TableBody>
            </Table>
        </TableWrapper>
    }
}

#[component]
fn RecentlyDeletedRow(row: DeletedCollectionRow, revision: RwSignal<u32>) -> impl IntoView {
    let id = row.id;
    let name = row.name.clone();
    let toast = expect_context::<ToastHandle>();
    let tree = expect_context::<CollectionTreeResource>().0;
    let busy = RwSignal::new(false);

    let restore = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let name = name.clone();
        spawn_local(async move {
            match crate::restore_collection(id).await {
                Ok(()) => {
                    // No Undo action on this toast — restore is the weaker,
                    // later path on purpose (specs/collection-deletion.md →
                    // step 5): it has no receipt of its own to reverse, and
                    // offering an "Undo the restore" would only re-delete it,
                    // which is a different operation with different copy, not
                    // this one played backwards.
                    toast.show(
                        ToastOptions::message(format!("Restored {name}")).kind(ToastKind::Success),
                    );
                    tree.refetch();
                    revision.update(|r| *r = r.wrapping_add(1));
                }
                Err(e) => {
                    busy.set(false);
                    toast.show(
                        ToastOptions::message(format!(
                            "Couldn't restore: {}",
                            crate::my::collection::message_of(&e)
                        ))
                        .kind(ToastKind::Error),
                    );
                }
            }
        });
    };

    view! {
        <TableRow {..} data-testid="recently-deleted-row" data-collection-id=id.to_string()>
            <TableCell class="p-2 font-medium">{row.name}</TableCell>
            <TableCell class="text-muted-foreground p-2 text-sm">
                {kind_label(row.kind)}
            </TableCell>
            <TableCell
                class="text-muted-foreground p-2 text-sm"
                {..}
                data-testid="recently-deleted-when"
            >
                {row.deleted_at}
            </TableCell>
            <TableCell class="p-2 text-right">
                <Button
                    variant=ButtonVariant::Outline
                    attr:data-testid="recently-deleted-restore"
                    attr:disabled=move || busy.get()
                    on:click=restore
                >
                    "Restore"
                </Button>
            </TableCell>
        </TableRow>
    }
}
