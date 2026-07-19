//! Label — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/label.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - the runtime-formatted named-peer classes (`peer-disabled/{for}:…`) are
//!   dropped: Tailwind only generates CSS for class strings it can see in
//!   source, so runtime-built variant names produce no styles — the static
//!   `peer-disabled:` pair is what actually works

use leptos::prelude::*;
use tw_merge::tw_merge;

#[component]
pub fn Label(
    #[prop(optional, into)] class: String,
    #[prop(optional, into)] html_for: String,
    children: Children,
) -> impl IntoView {
    let class = tw_merge!(
        "flex items-center gap-2 text-sm leading-none font-medium select-none group-data-[disabled=true]:pointer-events-none group-data-[disabled=true]:opacity-50",
        "peer-disabled:cursor-not-allowed peer-disabled:opacity-50",
        class
    );

    view! {
        <label class=class r#for=html_for data-name="Label">
            {children()}
        </label>
    }
}
