//! `/dev/components` — the UI component bench (specs/ui-component-bench.md).
//!
//! One page rendering every vendored Rust/UI component
//! (`app/src/components/ui/`) with representative variants, under a static
//! panel of the current theme tokens. Compiled only with the
//! `component-bench` cargo feature — dev entry points switch it on, release
//! builds leave it off.
//!
//! Adoption convention ("complete by construction"): vendoring a new
//! component includes, in the same commit, a demo fn in this module and one
//! line in [`SECTIONS`]. The page and jump-nav iterate the registry, so the
//! bench never lags the vendored set. Run the spec's per-adoption
//! verification checklist (SSR, hydration, ID stability, vendored assets,
//! native-webview positioning) against the new section before shipping it.

mod badge;
mod breadcrumb;
mod button;
mod card;
mod checkbox;
mod collapsible;
mod command;
mod context_menu;
mod dialog;
mod hover_card;
mod input;
mod input_group;
mod item;
mod kbd;
mod popover;
mod separator;
mod sheet;
mod skeleton;
mod sonner;
mod table;
mod theme_toggle;
mod toggle_group;

use leptos::prelude::*;

/// One bench section per vendored component.
struct BenchSection {
    /// Anchor id; the jump-nav links to `#<id>`.
    id: &'static str,
    title: &'static str,
    demo: fn() -> AnyView,
}

/// The section registry — one line per vendored component, in page order.
const SECTIONS: &[BenchSection] = &[
    BenchSection {
        id: "button",
        title: "Button",
        demo: button::demo,
    },
    BenchSection {
        id: "input",
        title: "Input",
        demo: input::demo,
    },
    BenchSection {
        id: "input-group",
        title: "Input group",
        demo: input_group::demo,
    },
    BenchSection {
        id: "badge",
        title: "Badge",
        demo: badge::demo,
    },
    BenchSection {
        id: "kbd",
        title: "Kbd",
        demo: kbd::demo,
    },
    BenchSection {
        id: "separator",
        title: "Separator",
        demo: separator::demo,
    },
    BenchSection {
        id: "checkbox",
        title: "Checkbox + Label",
        demo: checkbox::demo,
    },
    BenchSection {
        id: "toggle-group",
        title: "Toggle group",
        demo: toggle_group::demo,
    },
    BenchSection {
        id: "breadcrumb",
        title: "Breadcrumb",
        demo: breadcrumb::demo,
    },
    BenchSection {
        id: "skeleton",
        title: "Skeleton",
        demo: skeleton::demo,
    },
    BenchSection {
        id: "card",
        title: "Card",
        demo: card::demo,
    },
    BenchSection {
        id: "collapsible",
        title: "Collapsible",
        demo: collapsible::demo,
    },
    BenchSection {
        id: "item",
        title: "Item",
        demo: item::demo,
    },
    BenchSection {
        id: "dialog",
        title: "Dialog",
        demo: dialog::demo,
    },
    BenchSection {
        id: "popover",
        title: "Popover",
        demo: popover::demo,
    },
    BenchSection {
        id: "sheet",
        title: "Sheet",
        demo: sheet::demo,
    },
    BenchSection {
        id: "command",
        title: "Command",
        demo: command::demo,
    },
    BenchSection {
        id: "context-menu",
        title: "Context menu",
        demo: context_menu::demo,
    },
    BenchSection {
        id: "hover-card",
        title: "Hover card",
        demo: hover_card::demo,
    },
    BenchSection {
        id: "sonner",
        title: "Sonner (toaster)",
        demo: sonner::demo,
    },
    BenchSection {
        id: "table",
        title: "Table",
        demo: table::demo,
    },
    BenchSection {
        id: "theme-toggle",
        title: "Theme toggle",
        demo: theme_toggle::demo,
    },
];

/// The primary color tokens from `style/input.css`. `--radius` joins them in
/// the panel with a rounded-box swatch instead of a color swatch.
const COLOR_TOKENS: &[&str] = &[
    "--background",
    "--foreground",
    "--card",
    "--card-foreground",
    "--popover",
    "--popover-foreground",
    "--primary",
    "--primary-foreground",
    "--secondary",
    "--secondary-foreground",
    "--muted",
    "--muted-foreground",
    "--accent",
    "--accent-foreground",
    "--destructive",
    "--destructive-foreground",
    "--success",
    "--success-foreground",
    "--success-light",
    "--success-dark",
    "--warning",
    "--warning-foreground",
    "--warning-light",
    "--warning-dark",
    "--info",
    "--info-foreground",
    "--info-light",
    "--info-dark",
    "--border",
    "--input",
    "--ring",
];

/// The bench page: jump-nav, theme panel, one section per registry entry.
/// The light/dark toggle flips the `dark` class on `<html>` (session-only —
/// no cookie, unlike the vendored ThemeToggle): now that the app themes
/// globally with dark as the default, a container-scoped class can't
/// override the ancestor's variables, so the bench control drives the real
/// root and the panel + every section re-resolve in that mode.
#[component]
pub fn BenchPage() -> impl IntoView {
    let (dark, set_dark) = signal(crate::components::ui::theme_toggle::cookie_theme_is_dark());
    let flip = move |_| {
        let now = !dark.get_untracked();
        set_dark.set(now);
        #[cfg(feature = "hydrate")]
        if let Some(root) = leptos::tachys::dom::document().document_element() {
            let _ = if now {
                root.class_list().add_1("dark")
            } else {
                root.class_list().remove_1("dark")
            };
        }
    };

    view! {
        <div class="min-h-screen bg-background text-foreground">
            <div class="flex gap-8 p-8">
                <nav class="sticky top-8 hidden h-fit w-40 shrink-0 flex-col gap-1 text-sm sm:flex">
                    <span class="text-muted-foreground mb-1 font-medium">"Sections"</span>
                    <a class="hover:underline" href="#theme">
                        "Theme"
                    </a>
                    {SECTIONS
                        .iter()
                        .map(|s| {
                            view! {
                                <a class="hover:underline" href=format!("#{}", s.id)>
                                    {s.title}
                                </a>
                            }
                        })
                        .collect_view()}
                </nav>
                <main class="min-w-0 flex-1 space-y-12">
                    <header class="flex items-start justify-between gap-4">
                        <div>
                            <h1 class="text-2xl font-bold">"Component bench"</h1>
                            <p class="text-muted-foreground text-sm">
                                "Every vendored Rust/UI component, with the current theme tokens above it. See specs/ui-component-bench.md."
                            </p>
                        </div>
                        <button
                            class="hover:bg-muted shrink-0 rounded-md border px-3 py-1.5 text-sm"
                            on:click=flip
                        >
                            {move || if dark.get() { "Light mode" } else { "Dark mode" }}
                        </button>
                    </header>
                    <ThemePanel dark=dark />
                    {SECTIONS
                        .iter()
                        .map(|s| {
                            view! {
                                <section id=s.id class="scroll-mt-8 space-y-4">
                                    <h2 class="border-b pb-2 text-xl font-semibold">{s.title}</h2>
                                    {(s.demo)()}
                                </section>
                            }
                        })
                        .collect_view()}
                </main>
            </div>
        </div>
    }
}

/// The static theme panel: one row per primary token, resolved in the active
/// mode. Read-only by design — editing the theme means editing
/// `style/input.css` and reloading; that is the styling workflow.
#[component]
fn ThemePanel(dark: ReadSignal<bool>) -> impl IntoView {
    view! {
        <section id="theme" class="scroll-mt-8 space-y-4">
            <h2 class="border-b pb-2 text-xl font-semibold">"Theme"</h2>
            <p class="text-muted-foreground text-sm">
                "The tokens from style/input.css, resolved in the active mode. Read-only: edit the CSS and reload to change them."
            </p>
            <div class="rounded-md border">
                {COLOR_TOKENS
                    .iter()
                    .map(|t| view! { <TokenRow name=*t dark=dark swatch=SwatchKind::Color /> })
                    .collect_view()}
                <TokenRow name="--radius" dark=dark swatch=SwatchKind::Radius />
            </div>
        </section>
    }
}

/// How a token row previews its value.
#[derive(Clone, Copy, PartialEq)]
enum SwatchKind {
    Color,
    Radius,
}

/// One theme-panel row: swatch + token name + resolved value. The value is
/// read from the row's live computed style — the CSS stays the single source
/// of truth, so the panel can never drift from a Rust-side palette copy. SSR
/// renders an ellipsis placeholder; hydration fills the value in and re-reads
/// it whenever the mode toggles.
#[component]
fn TokenRow(name: &'static str, dark: ReadSignal<bool>, swatch: SwatchKind) -> impl IntoView {
    let row = NodeRef::<leptos::html::Div>::new();
    let (value, set_value) = signal(None::<String>);

    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        dark.track();
        let resolved = row.get().and_then(|el| {
            let style = web_sys::window()?.get_computed_style(&el).ok()??;
            style.get_property_value(name).ok()
        });
        set_value.set(resolved.map(|v| v.trim().to_owned()));
    });
    #[cfg(not(feature = "hydrate"))]
    let _ = (dark, set_value);

    let swatch_style = match swatch {
        SwatchKind::Color => format!("background: var({name})"),
        SwatchKind::Radius => format!("border-radius: var({name})"),
    };

    view! {
        <div
            node_ref=row
            class="flex items-center gap-3 border-b px-3 py-2 text-sm last:border-b-0"
        >
            <span class="bg-muted h-6 w-6 shrink-0 rounded-sm border" style=swatch_style></span>
            <code class="w-44 shrink-0">{name}</code>
            <code class="text-muted-foreground">
                {move || value.get().unwrap_or_else(|| "\u{2026}".to_owned())}
            </code>
        </div>
    }
}
