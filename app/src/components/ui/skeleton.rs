//! Skeleton — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/skeleton.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream: `leptos_ui`'s
//! `void!` swapped for the vendored clx.rs (nightly hazard), nothing else.

use leptos::prelude::*;

use super::clx::void;

const PULSE_ANIMATION: &str = "animate-pulse";

mod components {
    use super::*;
    void! {Skeleton, div, PULSE_ANIMATION, "rounded-md bg-muted"}
}

pub use components::*;
