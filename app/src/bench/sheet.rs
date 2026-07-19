//! Bench section for the vendored `sheet` (right + bottom variants).

use leptos::prelude::*;

use crate::components::ui::sheet::{
    Sheet, SheetBody, SheetContent, SheetDescription, SheetDirection, SheetHeader, SheetTitle,
    SheetTrigger,
};

pub fn demo() -> AnyView {
    view! {
        <div class="flex flex-wrap items-center gap-3">
            <Sheet id="bench-sheet-right">
                <SheetTrigger>"Right sheet"</SheetTrigger>
                <SheetContent aria_label="Filters">
                    <SheetBody>
                        <SheetHeader>
                            <SheetTitle>"Filters"</SheetTitle>
                            <SheetDescription>"The mobile filter slide-over chrome."</SheetDescription>
                        </SheetHeader>
                    </SheetBody>
                </SheetContent>
            </Sheet>
            <Sheet id="bench-sheet-bottom">
                <SheetTrigger>"Bottom sheet"</SheetTrigger>
                <SheetContent direction=SheetDirection::Bottom aria_label="Card preview">
                    <SheetBody>
                        <SheetHeader>
                            <SheetTitle>"Card preview"</SheetTitle>
                            <SheetDescription>"The touch card-preview sheet."</SheetDescription>
                        </SheetHeader>
                    </SheetBody>
                </SheetContent>
            </Sheet>
        </div>
    }
    .into_any()
}
