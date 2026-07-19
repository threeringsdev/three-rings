//! Scroll lock — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/hooks/use_scroll_lock.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - the `window.ScrollLock` JS-interop registration (`init()`) is dropped:
//!   our overlays are Leptos-driven and call [`lock`]/[`unlock`] directly —
//!   no inline scripts remain that would need the JS global
//! - let-chains rewritten for the 2021 edition
//! - the whole module is hydrate-only (`web_sys` DOM work); hosted builds
//!   compile no-op stubs so callers need no cfg at the call site

#[cfg(feature = "hydrate")]
mod imp {
    use std::cell::RefCell;

    use wasm_bindgen::JsCast;

    /// Component data-names excluded from scroll locking (internal
    /// scrollable areas).
    const EXCLUDED_DATA_NAMES: &[&str] = &[
        "ScrollArea",
        "CommandList",
        "SelectContent",
        "MultiSelectContent",
        "DropdownMenuContent",
        "ContextMenuContent",
    ];

    /// Data-names excluded when collecting fixed-position elements.
    const FIXED_EXCLUDED: &[&str] = &[
        "DropdownMenuContent",
        "MultiSelectContent",
        "ContextMenuContent",
    ];

    /// CSS selector for scrollable element candidates.
    const SCROLLABLE_SELECTOR: &str =
        r#"[style*="overflow"],[class*="overflow"],[class*="scroll"],main,aside,section,div"#;

    /// CSS selector for fixed-position element candidates.
    const FIXED_SELECTOR: &str = r#"[style*="fixed"],[class*="fixed"],header,nav,aside,[role="dialog"],[role="alertdialog"]"#;

    struct BodyStyles {
        position: String,
        top: String,
        width: String,
        overflow: String,
        padding_right: String,
    }

    struct ScrollableEntry {
        element: web_sys::HtmlElement,
        scroll_top: i32,
        overflow: String,
        overflow_y: String,
        padding_right: String,
    }

    struct FixedEntry {
        element: web_sys::HtmlElement,
        padding_right: String,
    }

    struct State {
        /// Overlays currently holding the lock (reference count).
        owners: u32,
        /// Bumped on every lock(); a pending delayed restore no-ops if the
        /// generation moved (close-then-reopen race).
        generation: u64,
        /// Whether the DOM is currently in the locked state (survives until
        /// the delayed restore actually runs).
        dom_locked: bool,
        window_scroll_y: f64,
        body_styles: Option<BodyStyles>,
        scrollable: Vec<ScrollableEntry>,
        fixed: Vec<FixedEntry>,
    }

    impl State {
        const fn new() -> Self {
            Self {
                owners: 0,
                generation: 0,
                dom_locked: false,
                window_scroll_y: 0.0,
                body_styles: None,
                scrollable: Vec::new(),
                fixed: Vec::new(),
            }
        }

        fn clear(&mut self) {
            self.dom_locked = false;
            self.window_scroll_y = 0.0;
            self.body_styles = None;
            self.scrollable.clear();
            self.fixed.clear();
        }
    }

    thread_local! {
        static STATE: RefCell<State> = const { RefCell::new(State::new()) };
    }

    fn is_excluded(el: &web_sys::Element) -> bool {
        if let Some(name) = el.get_attribute("data-name") {
            if EXCLUDED_DATA_NAMES.iter().any(|&n| n == name) {
                return true;
            }
        }
        for &name in EXCLUDED_DATA_NAMES {
            let sel = format!(r#"[data-name="{name}"]"#);
            if el.closest(&sel).ok().flatten().is_some() {
                return true;
            }
        }
        false
    }

    fn is_fixed_excluded(el: &web_sys::Element) -> bool {
        for &name in FIXED_EXCLUDED {
            let sel = format!(r#"[data-name="{name}"]"#);
            if el.closest(&sel).ok().flatten().is_some() {
                return true;
            }
        }
        false
    }

    fn set_style(style: &web_sys::CssStyleDeclaration, prop: &str, val: &str) {
        if val.is_empty() {
            let _ = style.remove_property(prop);
        } else {
            let _ = style.set_property(prop, val);
        }
    }

    fn parse_px(s: &str) -> f64 {
        s.trim_end_matches("px").parse::<f64>().unwrap_or(0.0)
    }

    pub fn lock() {
        // Reference-counted: every overlay registers; only the transition to
        // the first owner with an unlocked DOM does the style work. The
        // generation bump invalidates any pending delayed restore (reopen
        // during the exit animation keeps the DOM locked).
        let proceed = STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.owners += 1;
            s.generation += 1;
            if s.dom_locked {
                return false;
            }
            s.dom_locked = true;
            true
        });
        if !proceed {
            return;
        }

        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(document) = window.document() else {
            return;
        };
        let Some(body) = document.body() else { return };

        // ── READ PHASE ─────────────────────────────────────────────

        let window_scroll_y = window.scroll_y().unwrap_or(0.0);
        let inner_width = window
            .inner_width()
            .ok()
            .and_then(|w| w.as_f64())
            .unwrap_or(0.0);
        let scrollbar_width = inner_width - body.client_width() as f64;

        let body_style = body.style();
        let original_body = BodyStyles {
            position: body_style
                .get_property_value("position")
                .unwrap_or_default(),
            top: body_style.get_property_value("top").unwrap_or_default(),
            width: body_style.get_property_value("width").unwrap_or_default(),
            overflow: body_style
                .get_property_value("overflow")
                .unwrap_or_default(),
            padding_right: body_style
                .get_property_value("padding-right")
                .unwrap_or_default(),
        };

        let body_js: &wasm_bindgen::JsValue = body.as_ref();
        let doc_element = document.document_element();

        struct SRead {
            el: web_sys::HtmlElement,
            scroll_top: i32,
            overflow: String,
            overflow_y: String,
            padding_right: String,
            computed_padding: f64,
            el_scrollbar: i32,
        }

        let mut s_reads: Vec<SRead> = Vec::new();

        if let Ok(nodes) = document.query_selector_all(SCROLLABLE_SELECTOR) {
            for i in 0..nodes.length() {
                let Some(node) = nodes.item(i) else { continue };
                let Ok(element) = node.dyn_into::<web_sys::Element>() else {
                    continue;
                };

                let el_js: &wasm_bindgen::JsValue = element.as_ref();
                if el_js == body_js {
                    continue;
                }
                if let Some(ref de) = doc_element {
                    let de_js: &wasm_bindgen::JsValue = de.as_ref();
                    if el_js == de_js {
                        continue;
                    }
                }

                if is_excluded(&element) {
                    continue;
                }

                let Some(cs) = window.get_computed_style(&element).ok().flatten() else {
                    continue;
                };
                let ov = cs.get_property_value("overflow").unwrap_or_default();
                let ovy = cs.get_property_value("overflow-y").unwrap_or_default();
                let scrollable = matches!(ov.as_str(), "auto" | "scroll")
                    || matches!(ovy.as_str(), "auto" | "scroll");

                if !scrollable || element.scroll_height() <= element.client_height() {
                    continue;
                }

                let Ok(el) = element.dyn_into::<web_sys::HtmlElement>() else {
                    continue;
                };

                let st = el.style();
                let cp = cs
                    .get_property_value("padding-right")
                    .ok()
                    .map(|p| parse_px(&p))
                    .unwrap_or(0.0);

                s_reads.push(SRead {
                    scroll_top: el.scroll_top(),
                    overflow: st.get_property_value("overflow").unwrap_or_default(),
                    overflow_y: st.get_property_value("overflow-y").unwrap_or_default(),
                    padding_right: st.get_property_value("padding-right").unwrap_or_default(),
                    computed_padding: cp,
                    el_scrollbar: el.offset_width() - el.client_width(),
                    el,
                });
            }
        }

        struct FRead {
            el: web_sys::HtmlElement,
            original_pr: String,
            computed_padding: f64,
        }

        let mut f_reads: Vec<FRead> = Vec::new();

        if scrollbar_width > 0.0 {
            if let Ok(nodes) = document.query_selector_all(FIXED_SELECTOR) {
                for i in 0..nodes.length() {
                    let Some(node) = nodes.item(i) else { continue };
                    let Ok(element) = node.dyn_into::<web_sys::Element>() else {
                        continue;
                    };

                    let Some(cs) = window.get_computed_style(&element).ok().flatten() else {
                        continue;
                    };
                    if cs.get_property_value("position").unwrap_or_default() != "fixed" {
                        continue;
                    }
                    if is_fixed_excluded(&element) {
                        continue;
                    }

                    let Ok(el) = element.dyn_into::<web_sys::HtmlElement>() else {
                        continue;
                    };

                    let cp = cs
                        .get_property_value("padding-right")
                        .ok()
                        .map(|p| parse_px(&p))
                        .unwrap_or(0.0);

                    f_reads.push(FRead {
                        original_pr: el
                            .style()
                            .get_property_value("padding-right")
                            .unwrap_or_default(),
                        computed_padding: cp,
                        el,
                    });
                }
            }
        }

        // ── WRITE PHASE ────────────────────────────────────────────

        let _ = body_style.set_property("position", "fixed");
        let _ = body_style.set_property("top", &format!("-{window_scroll_y}px"));
        let _ = body_style.set_property("width", "100%");
        let _ = body_style.set_property("overflow", "hidden");

        if scrollbar_width > 0.0 {
            let _ = body_style.set_property("padding-right", &format!("{scrollbar_width}px"));

            for fr in &f_reads {
                let np = fr.computed_padding + scrollbar_width;
                let _ = fr
                    .el
                    .style()
                    .set_property("padding-right", &format!("{np}px"));
            }
        }

        for sr in &s_reads {
            let _ = sr.el.style().set_property("overflow", "hidden");
            if sr.el_scrollbar > 0 {
                let np = sr.computed_padding + sr.el_scrollbar as f64;
                let _ = sr
                    .el
                    .style()
                    .set_property("padding-right", &format!("{np}px"));
            }
        }

        STATE.with(|state| {
            let mut s = state.borrow_mut();
            s.window_scroll_y = window_scroll_y;
            s.body_styles = Some(original_body);
            s.scrollable = s_reads
                .into_iter()
                .map(|r| ScrollableEntry {
                    element: r.el,
                    scroll_top: r.scroll_top,
                    overflow: r.overflow,
                    overflow_y: r.overflow_y,
                    padding_right: r.padding_right,
                })
                .collect();
            s.fixed = f_reads
                .into_iter()
                .map(|r| FixedEntry {
                    element: r.el,
                    padding_right: r.original_pr,
                })
                .collect();
        });
    }

    pub fn unlock(delay_ms: u32) {
        let gen_at_schedule = STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.owners == 0 {
                return None;
            }
            s.owners -= 1;
            if s.owners > 0 {
                return None; // another overlay still holds the lock
            }
            Some(s.generation)
        });
        let Some(generation) = gen_at_schedule else {
            return;
        };

        let run = move || perform_unlock_if(generation);
        if delay_ms > 0 {
            leptos::prelude::set_timeout(
                run,
                std::time::Duration::from_millis(u64::from(delay_ms)),
            );
        } else {
            run();
        }
    }

    pub fn is_locked() -> bool {
        STATE.with(|s| s.borrow().dom_locked)
    }

    fn perform_unlock_if(generation: u64) {
        let Some(window) = web_sys::window() else {
            return;
        };

        STATE.with(|state| {
            let mut s = state.borrow_mut();
            // A newer lock() (reopen) or surviving owner supersedes this
            // scheduled restore.
            if s.generation != generation || s.owners > 0 || !s.dom_locked {
                return;
            }

            if let Some(body) = window.document().and_then(|d| d.body()) {
                if let Some(ref orig) = s.body_styles {
                    let st = body.style();
                    set_style(&st, "position", &orig.position);
                    set_style(&st, "top", &orig.top);
                    set_style(&st, "width", &orig.width);
                    set_style(&st, "overflow", &orig.overflow);
                    set_style(&st, "padding-right", &orig.padding_right);
                }
            }

            window.scroll_to_with_x_and_y(0.0, s.window_scroll_y);

            for entry in &s.scrollable {
                let st = entry.element.style();
                set_style(&st, "overflow", &entry.overflow);
                set_style(&st, "overflow-y", &entry.overflow_y);
                set_style(&st, "padding-right", &entry.padding_right);
                entry.element.set_scroll_top(entry.scroll_top);
            }

            for entry in &s.fixed {
                set_style(
                    &entry.element.style(),
                    "padding-right",
                    &entry.padding_right,
                );
            }

            s.clear();
        });
    }
}

#[cfg(feature = "hydrate")]
pub use imp::{is_locked, lock, unlock};

/// SSR stubs — overlays render closed markup server-side and never lock.
#[cfg(not(feature = "hydrate"))]
mod imp {
    pub fn lock() {}
    pub fn unlock(_delay_ms: u32) {}
    pub fn is_locked() -> bool {
        false
    }
}
#[cfg(not(feature = "hydrate"))]
pub use imp::{is_locked, lock, unlock};
