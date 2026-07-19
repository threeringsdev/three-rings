//! Bench section for the vendored `card`.

use leptos::prelude::*;

use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::card::{
    Card, CardAction, CardContent, CardDescription, CardFooter, CardHeader, CardSize, CardTitle,
};

pub fn demo() -> AnyView {
    view! {
        <div class="flex flex-wrap items-start gap-4">
            <Card class="max-w-xs">
                <CardHeader>
                    <CardTitle>"Trade Binder"</CardTitle>
                    <CardDescription>"6 cards · 12 copies"</CardDescription>
                    <CardAction>
                        <Button variant=ButtonVariant::Ghost size=ButtonSize::IconSm>"⋯"</Button>
                    </CardAction>
                </CardHeader>
                <CardContent>
                    <p class="text-sm">"The bulk box — sorted by set, roughly."</p>
                </CardContent>
                <CardFooter>
                    <Button variant=ButtonVariant::Outline size=ButtonSize::Sm>"Open"</Button>
                </CardFooter>
            </Card>
            <Card size=CardSize::Sm class="max-w-xs">
                <CardHeader>
                    <CardTitle>"Small card"</CardTitle>
                </CardHeader>
                <CardContent>
                    <p class="text-sm text-muted-foreground">"size=Sm density"</p>
                </CardContent>
            </Card>
        </div>
    }
    .into_any()
}
