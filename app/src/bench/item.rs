//! Bench section for the vendored `item`.

use leptos::prelude::*;

use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::item::{
    Item, ItemActions, ItemContent, ItemDescription, ItemGroup, ItemMedia, ItemMediaVariant,
    ItemSeparator, ItemSize, ItemTitle, ItemVariant,
};

pub fn demo() -> AnyView {
    view! {
        <ItemGroup class="max-w-sm gap-1">
            <Item size=ItemSize::Xs href="#item">
                <ItemMedia>
                    <span aria-hidden="true">"📥"</span>
                </ItemMedia>
                <ItemContent>
                    <ItemTitle>"Link row (hover feedback)"</ItemTitle>
                </ItemContent>
                <ItemActions>
                    <Badge variant=BadgeVariant::Muted size=BadgeSize::Sm>"7"</Badge>
                </ItemActions>
            </Item>
            <ItemSeparator />
            <Item variant=ItemVariant::Outline size=ItemSize::Sm>
                <ItemMedia variant=ItemMediaVariant::Icon>
                    <span aria-hidden="true">"◈"</span>
                </ItemMedia>
                <ItemContent>
                    <ItemTitle>"Outline + icon media"</ItemTitle>
                    <ItemDescription>"Static div row with a description line."</ItemDescription>
                </ItemContent>
            </Item>
            <Item variant=ItemVariant::Muted>
                <ItemContent>
                    <ItemTitle>"Muted, default size"</ItemTitle>
                </ItemContent>
            </Item>
        </ItemGroup>
    }
    .into_any()
}
