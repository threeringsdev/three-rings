//! Separator — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/separator.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `TwClass`/`TwVariant` derives hand-expanded into a plain enum + match
//!   (same class strings, no derive machinery)

use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SeparatorOrientation {
    #[default]
    Default,
    Vertical,
}

impl SeparatorOrientation {
    fn classes(self) -> &'static str {
        match self {
            Self::Default => "w-full h-[1px]",
            Self::Vertical => "h-full w-[1px]",
        }
    }
}

#[component]
pub fn Separator(
    #[prop(into, optional)] orientation: Signal<SeparatorOrientation>,
    #[prop(into, optional)] class: String,
) -> impl IntoView {
    let merged_class = Memo::new(move |_| {
        tw_merge::tw_merge!(
            "shrink-0 bg-border",
            orientation.get().classes(),
            class.clone()
        )
    });

    view! { <div class=merged_class role="separator" data-name="Separator" /> }
}
