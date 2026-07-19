//! Bench section for the vendored `theme_toggle`.
//!
//! Unlike every other section, this one is *live against the real page
//! theme*: clicking it flips the `dark` class on `<html>` and persists the
//! `tr_theme` override cookie — exactly what it will do in the app shell. The
//! bench-local toggle above only themes the bench container; this one themes
//! the world. Reload after toggling to verify the SSR side re-applies the
//! cookie.

use leptos::prelude::*;

use crate::components::ui::theme_toggle::ThemeToggle;

pub fn demo() -> AnyView {
    view! {
        <div class="flex items-center gap-4">
            <ThemeToggle />
            <p class="text-muted-foreground text-sm">
                "Flips the app-wide theme (class on <html> + tr_theme cookie; dark is the "
                "default). Reload to see SSR re-apply the persisted override."
            </p>
        </div>
    }
    .into_any()
}
