//! The catalog filter rail and its two-way binding to the query text
//! (specs/app-ui.md "`/catalog`", specs/catalog-search.md "One filter state,
//! two views over it").
//!
//! The contract, and every subtlety in this file follows from it:
//!
//! - **The query text stays canonical.** The rail is a *view over the query
//!   string*, never a second source of truth. It holds no state of its own: it
//!   reads the URL's `q` and every edit rewrites that string and navigates.
//!   So a rail edit and a typed edit are the same operation, and Back works
//!   across both.
//! - **Rail edits rewrite their own term and nothing else.** Terms the rail
//!   has no widget for — negations, `id:`, a second `c:` — survive byte for
//!   byte, because rewriting re-emits their original token text rather than
//!   re-serializing a parsed AST (see [`shared::search::parse_tokens`]).
//! - **A query the grammar rejects makes the rail inert, not wrong.** There is
//!   no way to reflect an unparseable query into widgets, and no way to rewrite
//!   one term of it without guessing at the broken one. Showing empty-but-
//!   editable widgets would silently delete the user's text on the next click.
//!
//! The pure half (everything above the components) is unit-tested; the widgets
//! are thin over it.

use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_query_map};
use leptos_router::NavigateOptions;
use shared::search::{facet_term, parse_tokens, quote_value, Cmp, ParseError, Pred, Term};

use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::label::Label;
use crate::components::ui::sheet::{Sheet, SheetClose, SheetContent, SheetDirection, SheetTrigger};

/// The rail's curated vocabulary — one variant per widget, and the unit that
/// [`rewrite`] replaces. Deliberately narrower than the grammar: `id:` and
/// negation have no widget and so are never owned by anything here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Text,
    Set,
    Color,
    Type,
    Rarity,
    ManaValue,
}

/// Which widget owns a term, if any.
///
/// Negated terms are owned by nobody on purpose: the rail's widgets are
/// checkboxes and text fields with no "not" affordance, so claiming `-t:land`
/// would mean the Type facet could only give it back as `t:land` — an edit
/// elsewhere in the rail would quietly invert the user's filter.
fn field_of(term: &Term) -> Option<Field> {
    if term.negated {
        return None;
    }
    Some(match term.pred {
        Pred::Name(_) => Field::Name,
        Pred::OracleText(_) => Field::Text,
        Pred::Set(_) => Field::Set,
        Pred::TypeLine(_) => Field::Type,
        Pred::Rarity(_) => Field::Rarity,
        Pred::Colors(_) | Pred::Colorless => Field::Color,
        Pred::ManaValue(..) => Field::ManaValue,
        Pred::Identity(_) => return None,
    })
}

/// Everything the rail's widgets display, read out of a query string.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RailState {
    /// The bare-word run, re-joined as typed (quoting included).
    pub name: String,
    pub text: String,
    /// Set codes as one comma-joined string — the widget is a text field, not
    /// a picker, until a `list_sets` adapter exists (see the spec Findings).
    pub set: String,
    pub colors: Vec<char>,
    /// `c:colorless` — a Color-field filter with no checkbox to show it (the
    /// wireframe's facet is the five colors). Tracked so it still counts.
    pub colorless: bool,
    pub types: Vec<String>,
    pub rarities: Vec<String>,
    pub mana_value: Option<(Cmp, f64)>,
}

impl RailState {
    /// How many filters this widget group is contributing — the section badge
    /// counts in the wireframes, and (summed) the mobile sheet's badge.
    pub fn count(&self, field: Field) -> usize {
        match field {
            Field::Name => usize::from(!self.name.is_empty()),
            Field::Text => usize::from(!self.text.is_empty()),
            Field::Set => self.set_codes().len(),
            Field::Color => self.colors.len() + usize::from(self.colorless),
            Field::Type => self.types.len(),
            Field::Rarity => self.rarities.len(),
            Field::ManaValue => usize::from(self.mana_value.is_some()),
        }
    }

    /// Total active filters — the mobile filter button's badge.
    pub fn total(&self) -> usize {
        [
            Field::Name,
            Field::Text,
            Field::Set,
            Field::Color,
            Field::Type,
            Field::Rarity,
            Field::ManaValue,
        ]
        .into_iter()
        .map(|f| self.count(f))
        .sum()
    }

    pub fn set_codes(&self) -> Vec<String> {
        split_codes(&self.set)
    }
}

/// Does this field own *every* matching term, or only the first one it sees?
///
/// Only the name box owns a run: bare words are collectively what it holds
/// (`lightning bolt` is two terms and one field). Every keyed facet owns just
/// its first term, because a repeat like `c:u c:r` is an AND that the rail's
/// single widget cannot express — so the extra is treated as hand-written text
/// and preserved. [`read`] and [`rewrite`] must agree on this, or an edit drops
/// something the rail told the user it wasn't touching.
fn owns_every_match(field: Field) -> bool {
    field == Field::Name
}

/// Serialize one name value back into the token the box should show, so that
/// what is displayed re-parses to exactly the same predicate.
fn name_token(v: &str) -> String {
    let round_trips = matches!(
        parse_tokens(v).as_deref(),
        Ok([only]) if !only.term.negated && matches!(&only.term.pred, Pred::Name(n) if n == v)
    );
    // Needs quoting otherwise: it has whitespace, reads as a keyed term, or
    // reads as a negation — each of which changes meaning if pasted in bare.
    if round_trips {
        v.to_string()
    } else {
        force_quote(v)
    }
}

/// Read a query string into rail state. `Err` means the grammar rejected it,
/// which is a normal mid-typing condition, not a bug — the caller renders the
/// rail inert rather than guessing.
///
/// Per [`owns_every_match`], a repeated facet key shows its first term only
/// and the rest stay hand-written text: `c:ur c:w` shows U+R and keeps `c:w`
/// verbatim through any later edit.
pub fn read(q: &str) -> Result<RailState, ParseError> {
    let tokens = parse_tokens(q)?;
    let mut st = RailState::default();
    let mut names: Vec<String> = Vec::new();
    let mut seen: Vec<Field> = Vec::new();

    for tok in &tokens {
        let Some(field) = field_of(&tok.term) else {
            continue;
        };
        if owns_every_match(field) {
            if let Pred::Name(v) = &tok.term.pred {
                // The *value*, re-serialized — not the raw token. `name:bolt`
                // and `bolt` are the same search, but echoing the raw form into
                // the box would re-serialize on the next edit as the literal
                // `"name:bolt"`, changing what is searched.
                names.push(name_token(v));
            }
            continue;
        }
        if seen.contains(&field) {
            continue;
        }
        seen.push(field);
        match &tok.term.pred {
            Pred::OracleText(v) => st.text = v.clone(),
            Pred::Set(codes) => st.set = codes.join(","),
            Pred::TypeLine(vals) => st.types = vals.clone(),
            Pred::Rarity(vals) => st.rarities = vals.clone(),
            Pred::Colors(cs) => st.colors = cs.clone(),
            // `c:colorless` owns the Color field but checks no box — the
            // wireframe's facet draws the five colors only. It still has to
            // *count*, or the badge reads 0 and the Reset button disappears on
            // a query that is very much filtered.
            Pred::Colorless => st.colorless = true,
            Pred::ManaValue(cmp, n) => st.mana_value = Some((*cmp, *n)),
            Pred::Name(_) | Pred::Identity(_) => unreachable!("not rail-owned"),
        }
    }
    st.name = names.join(" ");
    Ok(st)
}

/// Replace `field`'s term in `q` with `replacement` (or remove it when
/// `None`), leaving every other token exactly as the user typed it.
///
/// The replacement lands **in the position the old term held**, not appended.
/// Editing a filter must not make the query text reshuffle itself under the
/// cursor of someone who is also typing in the box.
///
/// Only what the widget actually *showed* is replaced ([`owns_every_match`]):
/// a repeated facet key like `c:u c:r` displays as U alone, so an edit rewrites
/// the first and leaves `c:r` standing. Clobbering it here while `read` claimed
/// not to own it is exactly the silent data loss the verbatim-preservation rule
/// exists to prevent.
pub fn rewrite(q: &str, field: Field, replacement: Option<String>) -> Result<String, ParseError> {
    let tokens = parse_tokens(q)?;
    let mut out: Vec<String> = Vec::new();
    let mut placed = false;

    for tok in &tokens {
        if field_of(&tok.term) == Some(field) && (!placed || owns_every_match(field)) {
            // The first hit takes the replacement. Further hits are dropped
            // only for a field that owns its whole run (the name box); for
            // every other field they were never the widget's to begin with,
            // so they fall through to the verbatim branch below.
            if !placed {
                placed = true;
                if let Some(r) = &replacement {
                    out.push(r.clone());
                }
            }
            continue;
        }
        out.push(tok.raw.clone());
    }
    if !placed {
        if let Some(r) = replacement {
            out.push(r);
        }
    }
    Ok(out.join(" "))
}

/// Comma-split a set-code field into codes, dropping blanks so a trailing
/// comma mid-typing doesn't produce the valueless term the grammar rejects.
fn split_codes(s: &str) -> Vec<String> {
    s.split(',')
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty())
        .collect()
}

/// Serialize what the name box holds back into bare-word terms.
///
/// The box's content *is* the bare-word run of the query, so it is re-tokenized
/// with the same tokenizer rather than pasted in whole. Anything that would
/// parse as a keyed term gets quoted: typing `t:instant` into the box labelled
/// "Card name" means a name containing that text, and it must not become a
/// type filter behind the user's back.
fn name_terms(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let Ok(tokens) = parse_tokens(trimmed) else {
        // Unclosed quote while typing — keep it as one literal phrase rather
        // than refusing the edit.
        return Some(force_quote(trimmed));
    };
    let out: Vec<String> = tokens
        .iter()
        .map(|tok| match (&tok.term.pred, tok.term.negated) {
            (Pred::Name(_), false) => tok.raw.clone(),
            _ => force_quote(&tok.raw),
        })
        .collect();
    (!out.is_empty()).then(|| out.join(" "))
}

/// Quote unconditionally (unlike [`quote_value`], which only quotes when it
/// must), so the result can only ever parse as a name term.
fn force_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', ""))
}

/// `mv` back to text: whole numbers lose the `.0` (`mv<=2`, not `mv<=2.0`),
/// because the query text is user-facing and round-trips through the box.
fn mana_value_term(cmp: Cmp, n: f64) -> String {
    let num = if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    };
    let op = match cmp {
        Cmp::Eq => ":",
        Cmp::Lt => "<",
        Cmp::Le => "<=",
        Cmp::Gt => ">",
        Cmp::Ge => ">=",
    };
    format!("mv{op}{num}")
}

fn cmp_from_str(s: &str) -> Cmp {
    match s {
        "<" => Cmp::Lt,
        "<=" => Cmp::Le,
        ">" => Cmp::Gt,
        ">=" => Cmp::Ge,
        _ => Cmp::Eq,
    }
}

fn cmp_to_str(c: Cmp) -> &'static str {
    match c {
        Cmp::Eq => "=",
        Cmp::Lt => "<",
        Cmp::Le => "<=",
        Cmp::Gt => ">",
        Cmp::Ge => ">=",
    }
}

/// The five WUBRG checkboxes, in Magic's canonical order.
const COLORS: [(char, &str); 5] = [
    ('W', "White"),
    ('U', "Blue"),
    ('B', "Black"),
    ('R', "Red"),
    ('G', "Green"),
];

/// The Type facet's curated values (wireframes). `t:` is a substring match, so
/// these are ordinary type-line words rather than a closed enum — the facet is
/// a shortcut for the common ones, not the whole vocabulary.
const TYPES: [&str; 5] = ["Creature", "Instant", "Sorcery", "Artifact", "Enchantment"];

const RARITIES: [&str; 4] = ["Common", "Uncommon", "Rare", "Mythic"];

const MANA_OPS: [&str; 5] = ["=", "<=", ">=", "<", ">"];

/// Every rail-owned field, in rail order — what "Reset" walks.
const FIELDS: [Field; 7] = [
    Field::Name,
    Field::Text,
    Field::Set,
    Field::Color,
    Field::Type,
    Field::Rarity,
    Field::ManaValue,
];

/// Drop every rail-owned term, keeping everything else. "Reset" clears the
/// *filters*, not the query: a `-t:land` the user hand-typed is not the rail's
/// to throw away.
pub fn reset(q: &str) -> Result<String, ParseError> {
    let mut out = q.to_string();
    for field in FIELDS {
        out = rewrite(&out, field, None)?;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Components — thin over the pure layer above.
// ---------------------------------------------------------------------------

/// Read the rail's view of the current URL query, and a committer that
/// rewrites one field of it and navigates.
///
/// Both halves read the URL, never a cached copy: the query bar, Back/Forward
/// and the rail all write the same string, so anything cached here would be a
/// second source of truth by another name.
fn use_rail_state() -> Memo<Result<RailState, ParseError>> {
    let query_map = use_query_map();
    let url_q = Memo::new(move |_| query_map.read().get("q").unwrap_or_default());
    Memo::new(move |_| read(&url_q.get()))
}

/// The committer. Called in each widget's own body rather than passed down as
/// a prop: `use_navigate` hands back a closure that is neither `Send` nor
/// `Sync`, so threading it through component props would force every rail
/// widget into local storage for no gain. The hooks it reads are plain context
/// lookups — calling them per widget costs nothing.
fn use_commit() -> impl Fn(Field, Option<String>) + Clone + 'static {
    let query_map = use_query_map();
    let go = use_navigate_query();
    move |field: Field, replacement: Option<String>| {
        let q = query_map.read_untracked().get("q").unwrap_or_default();
        let Ok(next) = rewrite(&q, field, replacement) else {
            // Unreachable through the UI (an unparseable query renders the
            // notice instead of widgets), but rewriting on a guess is exactly
            // how a user's text gets eaten — so refuse instead.
            return;
        };
        go(next);
    }
}

/// Navigate to a query the caller has already built. Shared by Reset and by
/// [`use_commit`] so the "replace, don't push" rule and the view-param
/// preservation live in exactly one place.
fn use_navigate_query() -> impl Fn(String) + Clone + 'static {
    let query_map = use_query_map();
    let navigate = use_navigate();
    move |next: String| {
        let params = query_map.read_untracked();
        let list_view = params.get(super::VIEW_PARAM).as_deref() == Some(super::LIST_VIEW);
        let was_searching = !params.get("q").unwrap_or_default().is_empty();
        drop(params);

        // History granularity is per *search session*, exactly as the query bar
        // does it (see `QueryBar::commit`): the first filter on a bare
        // `/catalog` pushes, so Back returns to browse-all; refining an
        // existing query replaces, so dragging down a facet list can't bury the
        // previous page under one entry per checkbox. The two surfaces edit the
        // same string, so they must agree on this or Back behaves differently
        // depending on which one you used last.
        navigate(
            &super::catalog_url(&next, list_view),
            NavigateOptions {
                replace: was_searching,
                ..Default::default()
            },
        );
    }
}

/// The desktop sidebar rail (wireframes: "Filter Rail").
#[component]
pub fn FilterRail() -> impl IntoView {
    view! { <RailBody heading_id="filter-rail" /> }
}

/// The mobile slide-over (wireframes: "Mobile — Catalog filter sheet"). The
/// trigger carries the active-filter badge the spec asks for.
#[component]
pub fn FilterSheet(#[prop(into)] result_count: Signal<Option<usize>>) -> impl IntoView {
    let state = use_rail_state();
    let open = RwSignal::new(false);
    let total = Signal::derive(move || state.get().map(|s| s.total()).unwrap_or(0));

    view! {
        <div class="md:hidden">
            <Sheet id="catalog-filters" open>
                <SheetTrigger variant=ButtonVariant::Outline size=ButtonSize::Sm>
                    <span aria-hidden="true">"⚙"</span>
                    "Filters"
                    <Show when=move || { total.get() > 0 }>
                        <span data-testid="filter-badge" class="ml-1.5">
                            <Badge variant=BadgeVariant::Secondary size=BadgeSize::Sm>
                                {move || total.get().to_string()}
                            </Badge>
                        </span>
                    </Show>
                </SheetTrigger>
                <SheetContent
                    direction=SheetDirection::Left
                    aria_label="Filters"
                    class="w-[85vw] max-w-sm"
                >
                    <RailBody heading_id="filter-sheet" />
                    <div class="mt-6">
                        <SheetClose variant=ButtonVariant::Default class="w-full">
                            {move || match result_count.get() {
                                Some(n) => format!("Show {n} results"),
                                None => "Show results".to_string(),
                            }}
                        </SheetClose>
                    </div>
                </SheetContent>
            </Sheet>
        </div>
    }
}

/// The widgets themselves — shared by the desktop rail and the mobile sheet so
/// the two surfaces cannot drift in vocabulary or behavior.
#[component]
fn RailBody(heading_id: &'static str) -> impl IntoView {
    let state = use_rail_state();
    let go = use_navigate_query();
    let query_map = use_query_map();
    // `ok` is the "query parses" projection every widget reads; `None` is the
    // inert state, not an empty rail.
    let ok = Signal::derive(move || state.get().ok());

    let reset_all = move |_| {
        let q = query_map.read_untracked().get("q").unwrap_or_default();
        if let Ok(next) = reset(&q) {
            go(next);
        }
    };

    view! {
        // The testid is the instance id: the desktop rail and the mobile sheet
        // render the same body, and the sheet's content is in the DOM even
        // while closed, so a shared testid would match two nodes on every page.
        <div data-testid=heading_id>
            <div class="mb-4 flex items-center justify-between">
                <h2 id=heading_id class="text-sm font-medium">
                    "Filters"
                </h2>
                <Show when=move || { ok.get().map(|s| s.total() > 0).unwrap_or(false) }>
                    <Button
                        variant=ButtonVariant::Ghost
                        size=ButtonSize::Sm
                        class="h-7 px-2 text-xs"
                        {..}
                        on:click=reset_all.clone()
                    >
                        "Reset"
                    </Button>
                </Show>
            </div>
            <Show
                when=move || ok.get().is_some()
                fallback=|| {
                    view! {
                        // No widget can honestly represent a query the grammar
                        // rejected, and rewriting one term of it would mean
                        // guessing which term is broken. Say so instead.
                        <p class="text-muted-foreground text-xs" data-testid="filter-rail-inert">
                            "Filters are unavailable while the search box holds an error."
                        </p>
                    }
                }
            >
                <div class="space-y-4">
                    <RailTextField
                        id=format!("{heading_id}-name")
                        label="Card name"
                        placeholder="e.g. Lightning Bolt"
                        field=Field::Name
                        value=Signal::derive(move || ok.get().map(|s| s.name).unwrap_or_default())
                        to_term=name_terms
                    />
                    <RailTextField
                        id=format!("{heading_id}-text")
                        label="Card text"
                        placeholder="e.g. draw a card"
                        field=Field::Text
                        value=Signal::derive(move || ok.get().map(|s| s.text).unwrap_or_default())
                        to_term=oracle_term
                    />
                    <RailSection
                        title="Set"
                        count=Signal::derive(move || {
                            ok.get().map(|s| s.count(Field::Set)).unwrap_or(0)
                        })
                        default_open=false
                    >
                        <RailTextField
                            id=format!("{heading_id}-set")
                            label="Set codes"
                            placeholder="e.g. mh3, lea"
                            field=Field::Set
                            value=Signal::derive(move || {
                                ok.get().map(|s| s.set).unwrap_or_default()
                            })
                            to_term=set_term
                            hide_label=true
                        />
                    </RailSection>
                    <RailFacet
                        title="Color"
                        field=Field::Color
                        options=COLORS
                            .iter()
                            .map(|(c, name)| {
                                (c.to_ascii_lowercase().to_string(), name.to_string())
                            })
                            .collect()
                        selected=Signal::derive(move || {
                            ok.get()
                                .map(|s| {
                                    s.colors
                                        .iter()
                                        .map(|c| c.to_ascii_lowercase().to_string())
                                        .collect()
                                })
                                .unwrap_or_default()
                        })
                        to_term=color_term
                        default_open=true
                    />
                    <RailFacet
                        title="Type"
                        field=Field::Type
                        options=TYPES.iter().map(|t| (t.to_ascii_lowercase(), t.to_string())).collect()
                        selected=Signal::derive(move || ok.get().map(|s| s.types).unwrap_or_default())
                        to_term=type_term
                        default_open=true
                    />
                    <RailFacet
                        title="Rarity"
                        field=Field::Rarity
                        options=RARITIES
                            .iter()
                            .map(|r| (r.to_ascii_lowercase(), r.to_string()))
                            .collect()
                        selected=Signal::derive(move || {
                            ok.get().map(|s| s.rarities).unwrap_or_default()
                        })
                        to_term=rarity_term
                        default_open=false
                    />
                    <ManaValueSection
                        value=Signal::derive(move || ok.get().and_then(|s| s.mana_value))
                        id=format!("{heading_id}-mv")
                    />
                </div>
            </Show>
        </div>
    }
}

// Serializers, as plain fns so the widgets can take them as props.
fn oracle_term(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| format!("o:{}", quote_value(t)))
}

fn set_term(s: &str) -> Option<String> {
    facet_term("s", &split_codes(s))
}

/// Colors concatenate (`c:ur`) rather than comma-separating: `c:` means "has
/// all of these", so its values are one letter-set, not an OR list.
fn color_term(vals: &[String]) -> Option<String> {
    (!vals.is_empty()).then(|| format!("c:{}", vals.concat()))
}

fn type_term(vals: &[String]) -> Option<String> {
    facet_term("t", vals)
}

fn rarity_term(vals: &[String]) -> Option<String> {
    facet_term("r", vals)
}

/// A labelled text field bound to one rail field.
///
/// It keeps a local signal for what is being typed and re-seeds it from the
/// URL only when the URL moved without us — the same rule (and the same
/// reason) as the query bar's field sync in the parent module.
#[component]
fn RailTextField(
    #[prop(into)] id: String,
    label: &'static str,
    placeholder: &'static str,
    field: Field,
    value: Signal<String>,
    to_term: fn(&str) -> Option<String>,
    #[prop(optional)] hide_label: bool,
) -> impl IntoView {
    let commit = use_commit();
    let initial = value.get_untracked();
    let text = RwSignal::new(initial.clone());
    let self_pushed = StoredValue::new(initial.clone());
    let pending = StoredValue::new(None::<leptos::leptos_dom::helpers::TimeoutHandle>);

    Effect::new(move |_| {
        let from_url = value.get();
        if from_url != self_pushed.get_value() {
            self_pushed.set_value(from_url.clone());
            text.set(from_url);
        }
    });

    let clear_pending = move || {
        pending.update_value(|h| {
            if let Some(h) = h.take() {
                h.clear();
            }
        });
    };
    // A timer still armed when the rail unmounts would navigate into a
    // torn-down router.
    on_cleanup(clear_pending);

    let on_input = move |_| {
        clear_pending();
        let commit = commit.clone();
        let handle = set_timeout_with_handle(
            move || {
                let raw = text.get_untracked();
                // Record what we are about to put in the URL, so the sync
                // effect above recognizes our own edit and leaves the field
                // (and the caret) alone.
                self_pushed.set_value(raw.clone());
                commit(field, to_term(&raw));
            },
            std::time::Duration::from_millis(super::SEARCH_DEBOUNCE_MS as u64),
        );
        pending.set_value(handle.ok());
    };

    view! {
        <div class="space-y-1.5">
            <Label html_for=id.clone() class=if hide_label { "sr-only" } else { "text-xs" }>
                {label}
            </Label>
            <Input
                id=id
                placeholder=placeholder
                bind_value=text
                class="h-8 text-sm"
                {..}
                on:input=on_input
            />
        </div>
    }
}

/// A collapsible rail section with the wireframe's active-count badge.
///
/// `<details>` rather than a JS disclosure: it collapses without hydration, is
/// keyboard-operable for free, and SSRs in the right state. Openness is seeded
/// once (the wireframe's defaults, plus "open if it already has filters") and
/// then left to the user — re-deriving it reactively would slam a section shut
/// under someone mid-click.
#[component]
fn RailSection(
    title: &'static str,
    count: Signal<usize>,
    default_open: bool,
    children: Children,
) -> impl IntoView {
    let open = default_open || count.get_untracked() > 0;
    let testid = format!("filter-count-{}", title.to_lowercase().replace(' ', "-"));
    view! {
        <details open=open class="group border-t pt-3">
            <summary class="flex cursor-pointer list-none items-center gap-2 text-sm font-medium">
                <span
                    aria-hidden="true"
                    class="text-muted-foreground transition-transform group-open:rotate-90"
                >
                    "›"
                </span>
                <span>{title}</span>
                <Show when=move || { count.get() > 0 }>
                    <span class="ml-auto" data-testid=testid.clone()>
                        <Badge variant=BadgeVariant::Secondary size=BadgeSize::Sm>
                            {move || count.get().to_string()}
                        </Badge>
                    </span>
                </Show>
            </summary>
            <div class="mt-3">{children()}</div>
        </details>
    }
}

/// A multi-select facet: checkboxes over a curated value list, serialized as
/// one term.
#[component]
fn RailFacet(
    title: &'static str,
    field: Field,
    options: Vec<(String, String)>,
    selected: Signal<Vec<String>>,
    to_term: fn(&[String]) -> Option<String>,
    default_open: bool,
) -> impl IntoView {
    let commit = use_commit();
    let count = Signal::derive(move || selected.read().len());
    view! {
        <RailSection title count default_open>
            <div class="space-y-2">
                {options
                    .into_iter()
                    .map(|(value, label)| {
                        let v = value.clone();
                        let is_on = Signal::derive(move || selected.read().contains(&v));
                        let commit = commit.clone();
                        let toggle = move || {
                            // Rebuild the whole selection and re-serialize: the
                            // term is one unit, so a toggle is a replacement of
                            // it, never an append of another term.
                            let mut next = selected.get_untracked();
                            match next.iter().position(|s| s == &value) {
                                Some(i) => {
                                    next.remove(i);
                                }
                                None => next.push(value.clone()),
                            }
                            commit(field, to_term(&next));
                        };
                        let on_label = toggle.clone();
                        view! {
                            <div class="flex items-center gap-2">
                                <Checkbox
                                    checked=is_on
                                    aria_label=label.clone()
                                    on_checked_change=Callback::new(move |_| toggle())
                                />
                                // A <span>, not a <label>: Checkbox renders a
                                // <button role="checkbox">, which `for=` cannot
                                // target. Clicking it forwards the toggle, and
                                // it cannot double-fire — a click on the
                                // checkbox never reaches a sibling.
                                <span
                                    class="cursor-pointer select-none text-sm"
                                    on:click=move |_| on_label()
                                >
                                    {label}
                                </span>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        </RailSection>
    }
}

/// Mana value: a comparison and a number, together one `mv` term.
#[component]
fn ManaValueSection(value: Signal<Option<(Cmp, f64)>>, #[prop(into)] id: String) -> impl IntoView {
    let commit = use_commit();
    let count = Signal::derive(move || usize::from(value.read().is_some()));
    let op = RwSignal::new(
        value
            .get_untracked()
            .map(|(c, _)| cmp_to_str(c).to_string())
            .unwrap_or_else(|| "=".into()),
    );
    let initial_num = value
        .get_untracked()
        .map(|(_, n)| mana_number(n))
        .unwrap_or_default();
    let num = RwSignal::new(initial_num.clone());

    // Follow the URL when it moves without us (Back, a typed edit, Reset).
    Effect::new(move |_| match value.get() {
        Some((c, n)) => {
            op.set(cmp_to_str(c).to_string());
            num.set(mana_number(n));
        }
        None => num.set(String::new()),
    });

    let apply = move || {
        let raw = num.get_untracked();
        // An empty (or half-typed) box is "no mana-value filter", not `mv:0` —
        // clearing the field has to remove the term, or the filter could never
        // be turned off from the rail.
        let term = match raw.trim().parse::<f64>() {
            Ok(n) => Some(mana_value_term(cmp_from_str(&op.get_untracked()), n)),
            Err(_) => None,
        };
        commit(Field::ManaValue, term);
    };
    let on_op = {
        let apply = apply.clone();
        move |ev| {
            op.set(event_target_value(&ev));
            apply();
        }
    };
    let on_num = move |_| apply();

    view! {
        <RailSection title="Mana value" count default_open=false>
            <div class="flex items-center gap-2">
                <select
                    aria-label="Mana value comparison"
                    class="border-input h-8 rounded-md border bg-transparent px-2 text-sm"
                    prop:value=move || op.get()
                    on:change=on_op
                >
                    {MANA_OPS
                        .iter()
                        .map(|o| {
                            view! {
                                <option value=*o selected=move || op.get() == *o>
                                    {*o}
                                </option>
                            }
                        })
                        .collect_view()}
                </select>
                <Label html_for=id.clone() class="sr-only">
                    "Mana value"
                </Label>
                <Input
                    id=id.clone()
                    r#type=InputType::Number
                    min="0"
                    step="1"
                    placeholder="any"
                    bind_value=num
                    class="h-8 text-sm"
                    {..}
                    on:change=on_num
                />
            </div>
        </RailSection>
    }
}

/// Render a mana value for the number box — whole numbers without the `.0`.
fn mana_number(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::search::parse;

    fn t(pred: Pred) -> Term {
        Term {
            negated: false,
            pred,
        }
    }

    #[test]
    fn reads_the_wireframe_query_into_widgets() {
        let st = read("t:instant c:ur cmc<=2").unwrap();
        assert_eq!(st.types, vec!["instant".to_string()]);
        assert_eq!(st.colors, vec!['U', 'R']);
        assert_eq!(st.mana_value, Some((Cmp::Le, 2.0)));
        // The wireframe's own badge counts.
        assert_eq!(st.count(Field::Type), 1);
        assert_eq!(st.count(Field::Color), 2);
        assert_eq!(st.count(Field::ManaValue), 1);
        assert_eq!(st.total(), 4);
    }

    #[test]
    fn bare_words_are_the_name_box() {
        let st = read("lightning bolt t:instant").unwrap();
        assert_eq!(st.name, "lightning bolt");
        assert_eq!(st.count(Field::Name), 1);
    }

    #[test]
    fn rail_unrecognized_terms_survive_a_rail_edit_verbatim() {
        // The spec's core promise: `id:` and negations have no widget, so an
        // edit elsewhere must leave them character-for-character intact.
        let q = "id:wu -t:land o:\"draw a card\" c:u";
        let out = rewrite(q, Field::Color, Some("c:ur".into())).unwrap();
        assert_eq!(out, "id:wu -t:land o:\"draw a card\" c:ur");
    }

    #[test]
    fn an_edit_replaces_in_place_rather_than_appending() {
        let out = rewrite("c:u bolt r:rare", Field::Color, Some("c:ur".into())).unwrap();
        assert_eq!(out, "c:ur bolt r:rare");
    }

    #[test]
    fn clearing_a_facet_removes_its_term_entirely() {
        // Not `c:` with no value — that is a parse error, and it would make
        // unchecking the last color break the whole query.
        let out = rewrite("c:ur bolt", Field::Color, None).unwrap();
        assert_eq!(out, "bolt");
        assert!(read(&out).is_ok());
    }

    #[test]
    fn a_facet_absent_from_the_query_appends() {
        let out = rewrite("bolt", Field::Rarity, facet_term("r", &["rare".into()])).unwrap();
        assert_eq!(out, "bolt r:rare");
    }

    #[test]
    fn only_the_first_term_of_a_field_is_owned() {
        // The second `c:` is not the widget's — the rail showed U alone — so an
        // edit rewrites the first and leaves the other standing. `read` and
        // `rewrite` disagreeing here is silent data loss (Codex review, high).
        let st = read("c:u c:r").unwrap();
        assert_eq!(st.colors, vec!['U']);
        assert_eq!(st.count(Field::Color), 1);
        let out = rewrite("c:u c:r", Field::Color, Some("c:w".into())).unwrap();
        assert_eq!(out, "c:w c:r");
        // ...and the same for a repeated key that is a different alias.
        let out = rewrite("t:instant type:land", Field::Type, Some("t:sorcery".into())).unwrap();
        assert_eq!(out, "t:sorcery type:land");
    }

    #[test]
    fn the_name_run_is_owned_whole() {
        // The exception to the rule above: bare words are collectively the name
        // box, so rewriting it replaces all of them rather than only the first.
        let out = rewrite(
            "lightning bolt t:instant",
            Field::Name,
            Some("shock".into()),
        )
        .unwrap();
        assert_eq!(out, "shock t:instant");
    }

    #[test]
    fn colorless_counts_even_though_it_has_no_checkbox() {
        // `c:colorless` is a real Color filter the wireframe's five-checkbox
        // facet cannot draw. Counting it 0 would hide the Reset button and the
        // mobile badge on a filtered query (Codex review, medium).
        let st = read("c:colorless").unwrap();
        assert!(st.colorless);
        assert!(st.colors.is_empty());
        assert_eq!(st.count(Field::Color), 1);
        assert_eq!(st.total(), 1);
        // It is rail-owned, so picking a color replaces it and Reset clears it.
        assert_eq!(
            rewrite("c:colorless", Field::Color, Some("c:u".into())).unwrap(),
            "c:u"
        );
        assert_eq!(reset("c:colorless bolt").unwrap(), "");
    }

    #[test]
    fn the_name_box_shows_a_value_it_can_write_back() {
        // `name:bolt` and `bolt` are the same search. Echoing the raw token
        // would re-serialize on the next edit as the literal `"name:bolt"` —
        // a different search than the user asked for (Codex review, medium).
        let st = read("name:bolt").unwrap();
        assert_eq!(st.name, "bolt");
        let back = rewrite("name:bolt", Field::Name, name_terms(&st.name)).unwrap();
        assert_eq!(back, "bolt");
        assert_eq!(read(&back).unwrap().name, "bolt");
    }

    #[test]
    fn negated_terms_are_never_owned() {
        let st = read("-t:land -bolt").unwrap();
        assert_eq!(st.types, Vec::<String>::new());
        assert_eq!(st.name, "");
        assert_eq!(st.total(), 0);
        // ...and so an edit cannot touch them.
        let out = rewrite("-t:land", Field::Type, Some("t:creature".into())).unwrap();
        assert_eq!(out, "-t:land t:creature");
    }

    #[test]
    fn multi_select_serializes_to_comma_or() {
        let out = rewrite(
            "bolt",
            Field::Type,
            facet_term("t", &["instant".into(), "sorcery".into()]),
        )
        .unwrap();
        assert_eq!(out, "bolt t:instant,sorcery");
        assert_eq!(read(&out).unwrap().types, vec!["instant", "sorcery"]);
    }

    #[test]
    fn the_name_box_cannot_smuggle_in_a_keyed_term() {
        // Typing `t:instant` into the field labelled "Card name" means a name
        // containing that text — quoting is what keeps it a name term.
        let q = name_terms("t:instant").unwrap();
        assert_eq!(q, "\"t:instant\"");
        assert_eq!(read(&q).unwrap().name, "\"t:instant\"");
        // And it really is a name search, not a type filter.
        assert_eq!(read(&q).unwrap().types, Vec::<String>::new());

        // A leading `-` is the same trap by another route: unquoted it would
        // become a negation, inverting the filter the user typed (Codex e2e
        // mutation pass — the keyed-term case alone did not cover it).
        let neg = name_terms("-bolt").unwrap();
        assert_eq!(neg, "\"-bolt\"");
        assert_eq!(parse(&neg).unwrap(), vec![t(Pred::Name("-bolt".into()))]);
    }

    #[test]
    fn the_name_box_round_trips_quoted_phrases() {
        let q = rewrite("", Field::Name, name_terms("\"fire // ice\"")).unwrap();
        assert_eq!(q, "\"fire // ice\"");
        assert_eq!(read(&q).unwrap().name, "\"fire // ice\"");
        // Re-writing what was read back out is a fixed point — otherwise the
        // query text would drift on every unrelated rail click.
        let again = rewrite(&q, Field::Name, name_terms(&read(&q).unwrap().name)).unwrap();
        assert_eq!(again, q);
    }

    #[test]
    fn reset_clears_the_filters_but_not_the_query() {
        // "Reset" is the rail's button, so it clears the rail's terms. A
        // hand-typed `-t:land` or `id:wu` is not the rail's to throw away.
        let out = reset("bolt c:ur t:instant r:rare mv<=2 s:mh3 o:draw -t:land id:wu").unwrap();
        assert_eq!(out, "-t:land id:wu");
        assert_eq!(read(&out).unwrap().total(), 0);
    }

    #[test]
    fn an_unparseable_query_refuses_rather_than_guessing() {
        assert!(read("pow>3").is_err());
        assert!(rewrite("pow>3", Field::Color, Some("c:u".into())).is_err());
    }

    #[test]
    fn mana_value_terms_stay_integral() {
        assert_eq!(mana_value_term(Cmp::Le, 2.0), "mv<=2");
        assert_eq!(mana_value_term(Cmp::Eq, 3.0), "mv:3");
        assert_eq!(mana_value_term(Cmp::Gt, 0.5), "mv>0.5");
        // Round trip through the grammar.
        let q = mana_value_term(Cmp::Ge, 4.0);
        assert_eq!(read(&q).unwrap().mana_value, Some((Cmp::Ge, 4.0)));
    }

    #[test]
    fn set_codes_tolerate_mid_typing_commas() {
        let st = RailState {
            set: "mh3, ,lea,".into(),
            ..Default::default()
        };
        assert_eq!(st.set_codes(), vec!["mh3", "lea"]);
        let q = rewrite("", Field::Set, facet_term("s", &st.set_codes())).unwrap();
        assert_eq!(q, "s:mh3,lea");
    }

    #[test]
    fn colors_concatenate_rather_than_comma_separating() {
        // `c:` means "has ALL of these", so its values are one letter-set —
        // `c:ur`, not `c:u,r`, which the grammar rejects outright. Comma-OR is
        // right for every *other* facet, which is exactly why this one needs
        // its own guard (survived the first Codex mutation pass).
        assert_eq!(color_term(&["u".into(), "r".into()]), Some("c:ur".into()));
        assert_eq!(color_term(&[]), None);
        let q = color_term(&["u".into(), "r".into()]).unwrap();
        assert_eq!(read(&q).unwrap().colors, vec!['U', 'R']);
    }

    #[test]
    fn quote_value_is_only_used_where_it_is_needed() {
        assert_eq!(quote_value("mh3"), "mh3");
        assert_eq!(quote_value("draw a card"), "\"draw a card\"");
    }
}
