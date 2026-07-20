//! Input — copied from the Rust/UI registry (rust-ui/ui
//! `app_crates/registry/src/ui/input.rs` @ 43e1e32, MIT) per
//! specs/ui-components.md. Ours now; deviations from upstream:
//! - `strum::AsRefStr` replaced with a hand-written `as_str` (strum is not a
//!   workspace dependency and a 15-arm match doesn't earn one)

use leptos::html;
use leptos::prelude::*;
use tw_merge::tw_merge;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum InputType {
    #[default]
    Text,
    Email,
    Password,
    Number,
    Tel,
    Url,
    Search,
    Date,
    Time,
    DatetimeLocal,
    Month,
    Week,
    Color,
    File,
    Hidden,
}

impl InputType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Email => "email",
            Self::Password => "password",
            Self::Number => "number",
            Self::Tel => "tel",
            Self::Url => "url",
            Self::Search => "search",
            Self::Date => "date",
            Self::Time => "time",
            Self::DatetimeLocal => "datetime-local",
            Self::Month => "month",
            Self::Week => "week",
            Self::Color => "color",
            Self::File => "file",
            Self::Hidden => "hidden",
        }
    }
}

#[component]
pub fn Input(
    // Styling
    #[prop(into, optional)] class: String,

    // Common HTML attributes
    #[prop(default = InputType::default())] r#type: InputType,
    #[prop(into, optional)] placeholder: Option<String>,
    #[prop(into, optional)] name: Option<String>,
    #[prop(into, optional)] id: Option<String>,
    #[prop(into, optional)] title: Option<String>,
    #[prop(into, optional)] autocomplete: Option<String>,
    #[prop(optional)] disabled: bool,
    #[prop(optional)] readonly: bool,
    #[prop(optional)] required: bool,
    #[prop(optional)] autofocus: bool,
    #[prop(optional)] minlength: Option<u16>,

    // Number input attributes
    #[prop(into, optional)] min: Option<String>,
    #[prop(into, optional)] max: Option<String>,
    #[prop(into, optional)] step: Option<String>,

    // Two-way binding (like bind:value)
    #[prop(into, optional)] bind_value: Option<RwSignal<String>>,

    // Ref for direct DOM access
    #[prop(optional)] node_ref: NodeRef<html::Input>,
) -> impl IntoView {
    let merged_class = tw_merge!(
        "text-foreground file:text-foreground placeholder:text-muted-foreground selection:bg-primary selection:text-primary-foreground dark:bg-input/30 border-input flex h-9 w-full min-w-0 rounded-md border bg-transparent px-3 py-1 text-base shadow-xs transition-[color,box-shadow] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm",
        "focus-visible:border-ring focus-visible:ring-ring/50",
        "focus-visible:ring-2",
        "aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
        "read-only:bg-muted",
        class
    );

    let type_str = r#type.as_str();

    match bind_value {
        // `bind:value` is a *client-side* binding — it drives the DOM property
        // and renders no `value` attribute — so an SSR'd input comes back empty
        // and only fills in once wasm lands. On a shared link that reads as
        // data loss. Seeding the attribute here rather than at each call site
        // is deliberate: the trap is invisible (the field just looks empty),
        // every SSR'd form re-inherits it, and five callers had already grown
        // their own copy of this workaround before it was fixed in one place.
        //
        // Set once, not reactively: after hydration the property that
        // `bind:value` owns is what the browser shows, and a reactive attribute
        // would be a second writer racing it.
        Some(signal) => {
            let ssr_seed = signal.get_untracked();
            view! {
                <input
                    data-name="Input"
                    type=type_str
                    class=merged_class
                    placeholder=placeholder
                    name=name
                    id=id
                    title=title
                    autocomplete=autocomplete
                    disabled=disabled
                    readonly=readonly
                    required=required
                    autofocus=autofocus
                    minlength=minlength
                    min=min
                    max=max
                    step=step
                    value=ssr_seed
                    bind:value=signal
                    node_ref=node_ref
                />
            }
            .into_any()
        }
        None => view! {
            <input
                data-name="Input"
                type=type_str
                class=merged_class
                placeholder=placeholder
                name=name
                id=id
                title=title
                autocomplete=autocomplete
                disabled=disabled
                readonly=readonly
                required=required
                autofocus=autofocus
                minlength=minlength
                min=min
                max=max
                step=step
                node_ref=node_ref
            />
        }
        .into_any(),
    }
}
