//! Bench section for `components/ui/table.rs`. The table was runtime-verified
//! in the architecture spike (`/cards`); this section keeps its variants
//! reviewable: sticky header, scrolling wrapper, selected-row state, footer.

use leptos::prelude::*;

use crate::components::ui::table::*;

/// Static demo rows — enough of them that the wrapper's `max-h-96` scrolls,
/// which is what exercises the sticky header.
const ROWS: &[(&str, &str, &str, u32)] = &[
    ("Lightning Bolt", "2XM", "Common", 4),
    ("Counterspell", "MH2", "Common", 4),
    ("Swords to Plowshares", "OTC", "Uncommon", 3),
    ("Birds of Paradise", "FDN", "Rare", 2),
    ("Demonic Tutor", "UMA", "Rare", 1),
    ("Rhystic Study", "JMP", "Rare", 1),
    ("Sol Ring", "C21", "Uncommon", 6),
    ("Arcane Signet", "FDN", "Common", 5),
    ("Cultivate", "M21", "Common", 4),
    ("Beast Within", "MOC", "Uncommon", 3),
    ("Fact or Fiction", "DMR", "Uncommon", 2),
    ("Craterhoof Behemoth", "J22", "Mythic", 1),
];

pub(super) fn demo() -> AnyView {
    let total: u32 = ROWS.iter().map(|(_, _, _, qty)| qty).sum();

    view! {
        <div class="space-y-2">
            <p class="text-muted-foreground text-sm">
                "Caption, sticky header in a scrolling wrapper (max-h-96), one row in the data-[state=selected] state, footer."
            </p>
            <TableWrapper>
                <Table>
                    <TableCaption>"A static demo collection."</TableCaption>
                    <TableHeader>
                        <TableRow>
                            <TableHead>"Name"</TableHead>
                            <TableHead>"Set"</TableHead>
                            <TableHead>"Rarity"</TableHead>
                            <TableHead class="text-right">"Qty"</TableHead>
                        </TableRow>
                    </TableHeader>
                    <TableBody>
                        {ROWS
                            .iter()
                            .enumerate()
                            .map(|(i, (name, set, rarity, qty))| {
                                let selected = if i == 2 { Some("selected") } else { None };
                                view! {
                                    <TableRow {..} data-state=selected>
                                        <TableCell>{*name}</TableCell>
                                        <TableCell>{*set}</TableCell>
                                        <TableCell>{*rarity}</TableCell>
                                        <TableCell class="text-right">{*qty}</TableCell>
                                    </TableRow>
                                }
                            })
                            .collect_view()}
                    </TableBody>
                    <TableFooter>
                        <TableRow>
                            <TableCell>"Total"</TableCell>
                            <TableCell>""</TableCell>
                            <TableCell>""</TableCell>
                            <TableCell class="text-right">{total}</TableCell>
                        </TableRow>
                    </TableFooter>
                </Table>
            </TableWrapper>
        </div>
    }
    .into_any()
}
