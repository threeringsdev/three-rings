//! `clx!` — a component-from-classes macro, vendored from Rust/UI's
//! `leptos_ui` 0.3.22 (MIT, https://github.com/rust-ui/ui). Vendored rather
//! than depended on: the published crate enables `leptos/nightly`, which
//! would poison our stable-toolchain build via feature unification. The
//! macro itself is stable-safe. Upstream also ships `void!`/`transition!`
//! variants — add them from the same source when a component needs them.

/// Creates a `#[component]` wrapping `$element` with tailwind class merging:
/// the caller's `class` prop is tw-merged over the base classes, so callers
/// can override without class conflicts.
macro_rules! clx {
    ($name:ident, $element:ident, $($base_class:expr),+ $(,)?) => {
        #[component]
        pub fn $name(
            #[prop(into, optional)] class: String,
            children: Children,
        ) -> impl IntoView {
            let merged_classes = tw_merge::tw_merge!(tw_merge::tw_join!($($base_class),+), class);

            view! {
                <$element
                    class=merged_classes
                    data-name=stringify!($name)
                >
                    {children()}
                </$element>
            }
        }
    };
}

/// `void!` — the self-closing-element sibling of [`clx!`], vendored from the
/// same `leptos_ui` source: identical class merging, no children.
macro_rules! void {
    ($name:ident, $element:ident, $($base_class:expr),+ $(,)?) => {
        #[component]
        pub fn $name(
            #[prop(into, optional)] class: String,
        ) -> impl IntoView {
            let merged_classes = tw_merge::tw_merge!(tw_merge::tw_join!($($base_class),+), class);

            view! {
                <$element
                    class=merged_classes
                    data-name=stringify!($name)
                />
            }
        }
    };
}

pub(crate) use clx;
pub(crate) use void;
