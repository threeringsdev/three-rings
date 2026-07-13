// Deeply nested views (e.g. /login's stacked Show cards) overflow rustc's
// default type-query depth when computing the hydrate layout — and the
// limit is compiler-version sensitive (built locally, failed on CI's rustc).
#![recursion_limit = "256"]

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use app::*;
    // initializes logging using the `log` crate
    _ = console_log::init_with_level(log::Level::Debug);
    console_error_panic_hook::set_once();

    leptos::mount::hydrate_body(App);
}
