//! Table — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/table.rs`, MIT) per specs/ui-components.md.
//! Ours now; deviations from upstream: dropped the unrelated Card* helpers
//! it carried, and removed a malformed class fragment in `TableCell`.

use leptos::prelude::*;

use super::clx::clx;

clx! {TableWrapper, div, "overflow-auto rounded-md border max-h-96"}
clx! {Table, table, "w-full max-w-7xl text-sm caption-bottom"}
clx! {TableCaption, caption, "mt-4 text-sm text-muted-foreground"}
clx! {TableHeader, thead, "[&_tr]:border-b sticky top-0 z-10 bg-card"}
clx! {TableRow, tr, "border-b transition-colors data-[state=selected]:bg-muted hover:bg-muted/50"}
clx! {TableHead, th, "h-10 px-2 text-left align-middle font-medium text-muted-foreground [&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-[2px]"}
clx! {TableBody, tbody, "[&_tr:last-child]:border-0"}
clx! {TableCell, td, "p-4 align-middle [&:has([role=checkbox])]:pr-0"}
clx! {TableFooter, tfoot, "font-medium border border-t bg-muted/50 [&>tr]:last:border-b-0"}
