//! The shared vocabulary for the three arms every data surface has besides its
//! happy path — **nothing to show**, **the read failed**, **still loading** —
//! and the one rule this repo keeps relearning about them: *an arm must not
//! claim more than the app knows.*
//!
//! Three things are worth knowing before editing this file.
//!
//! **The banner decides its own affordances, from the error.** Every surface
//! used to hand-roll `<p role="alert" class="border-destructive/40 …">`, five
//! copies of one class string, each offering a different amount of nothing: the
//! catalog's paging arm offered a way home, `/my/all`'s offered none, and
//! `/cards/:id`'s "we couldn't load this card" was a page with no link on it at
//! all. [`ErrorNote`] is that banner, once — and it takes the *error*, not a
//! pre-rendered message, because which affordances are honest depends on which
//! failure it is ([`Failure`]). "Try again" over a stale `?cursor=` re-sends the
//! same bad request and fails identically; "Try again" over an unreachable Neon
//! is the whole fix. Offering both everywhere would make one of them a lie on
//! every page.
//!
//! **`unauthorized:` is its own arm, and it is the one that has bitten a human.**
//! An expired session used to render the raw string `unauthorized: invalid
//! token` inside a red box — which reads as a page bug, and was mistaken for one
//! (specs/app-ui.md → the e2e cookie trap). It is not a failure of the page: it
//! is a failure of the *session*, whose fix is a sign-in, so it gets that
//! sentence and that link.
//!
//! **P6-083: typed dispatch, string fallback.** [`describe`] used to classify
//! every failure by parsing the `ApiError` `Display` prefix out of a flattened
//! `ServerFnError<String>::ServerError` message — the wire carried nothing
//! richer. The server-fn wire now carries the typed `shared::ApiError` variant
//! itself (`ServerFnError<shared::ApiError>::WrappedServerError`), so
//! [`describe`] matches on the variant directly and [`classify`]'s string
//! parsing is now the fallback for genuinely-untyped transport failures (a
//! dropped fetch, a deserialization error) that never carried an `ApiError` to
//! begin with.
//!
//! **[`Tone`] exists so a badge cannot say the wrong kind of nothing.** An empty
//! needs list is *good news* (you hold every copy you want); a `/my` root
//! standing on [`fallback_rows`](crate::my::root::fallback_rows) is *partial*
//! (one read failed, the rest works); the catalog's dimmed last-good page is
//! *not current*. Three genuinely different claims, so three tones, each mapped
//! to exactly one of the `success`/`warning`/`info` token families — and to the
//! tokens, never to hand-picked colors: those families were tuned for ≥ 4.5:1 on
//! every enabled pair (style/input.css, with four deliberate deviations from
//! upstream).

use leptos::prelude::*;
use leptos::server_fn::ServerFnError;

use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};

/// What a failed read tells us about whether *this page* can do anything about
/// it — as far as the wire lets us tell.
///
/// The server-fn wire now carries the typed [`shared::ApiError`] variant
/// (P6-083), so [`describe`] matches on it directly; the `Display`-prefix
/// parsing in [`classify`] survives only as the fallback for `ServerFnError`
/// variants that never carried an `ApiError` at all (a dropped fetch, a
/// deserialization failure). The fallback's default is deliberately
/// [`Failure::Transport`]: an offline phone on the native backend is the
/// *ordinary* failure, and offering a retry that turns out to be useless costs
/// a click, while withholding one that would have worked leaves a dead end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The thing addressed is not there — a link to a deleted collection, an id
    /// that was never yours (the API deliberately answers 404 for both, so the
    /// copy has to cover both). Its own class rather than part of [`Self::Request`]
    /// for a plain reason: `ApiError::NotFound` carries a bare noun by convention
    /// across 21 call sites, so "Couldn't load this collection: collection" is
    /// what appending the detail produces. This arm states the situation instead.
    Missing,
    /// The request itself is wrong and will stay wrong — a stale `?cursor=`, a
    /// query the grammar rejects, an op the server refuses. A retry re-sends it
    /// verbatim, so the honest affordance is a way *out*, not a way to do it
    /// again. Unlike `Missing`, these details *are* sentences ("invalid cursor").
    Request,
    /// The session, not the page. Signing in again is the fix; nothing else on
    /// this surface is.
    Session,
    /// Something on the way to the data broke. Retrying is exactly the right
    /// thing to offer.
    Transport,
}

impl Failure {
    /// Whether a "Try again" on this failure could plausibly do anything.
    pub fn retryable(self) -> bool {
        matches!(self, Failure::Transport)
    }

    /// The `data-failure` value — the seam a test reads to check that an arm
    /// classified its error the way it claims to have.
    pub fn slug(self) -> &'static str {
        match self {
            Failure::Missing => "missing",
            Failure::Request => "request",
            Failure::Session => "session",
            Failure::Transport => "transport",
        }
    }

    /// The typed counterpart of [`classify`]'s string-prefix table — same
    /// grouping (`conflict`/`forbidden`/`validation` are the request's fault,
    /// `not_found` names the thing, `unauthorized` names the session,
    /// `upstream` is breakage worth retrying), read off the variant instead of
    /// its `Display` text.
    fn of_api_error(e: &shared::ApiError) -> Self {
        match e {
            shared::ApiError::NotFound(_) => Failure::Missing,
            shared::ApiError::Unauthorized(_) => Failure::Session,
            shared::ApiError::Forbidden(_)
            | shared::ApiError::Conflict(_)
            | shared::ApiError::Validation(_) => Failure::Request,
            shared::ApiError::Upstream(_) => Failure::Transport,
        }
    }
}

/// Classify a wire message by its `ApiError` `Display` prefix, and strip the
/// prefix off the human half.
///
/// `conflict` / `forbidden` / `validation` are statements about the request,
/// `not found` about the thing, `unauthorized` about the session. Everything
/// else — `upstream` included, plus any non-`ServerError` transport variant whose
/// text carries no prefix at all — is treated as breakage worth retrying.
pub fn classify(raw: &str) -> (Failure, &str) {
    if let Some(rest) = raw.strip_prefix("not found: ") {
        return (Failure::Missing, rest);
    }
    for prefix in ["conflict: ", "forbidden: ", "validation: "] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            return (Failure::Request, rest);
        }
    }
    if let Some(rest) = raw.strip_prefix("unauthorized: ") {
        return (Failure::Session, rest);
    }
    match raw.strip_prefix("upstream: ") {
        Some(rest) => (Failure::Transport, rest),
        None => (Failure::Transport, raw),
    }
}

/// Classify a server-fn error, typed variant first.
///
/// **P6-083.** A `WrappedServerError(ApiError)` — every read that goes through
/// `crate::api_err` — is classified by matching the variant directly
/// ([`Failure::of_api_error`]), no string parsing involved. Anything else
/// (`ServerError`, `Request`, `Deserialization`, …) is a `ServerFnError`
/// variant that never carried a typed `ApiError` to begin with — those fall
/// back to [`classify`]'s `Display`-prefix table, unchanged from before this
/// task.
pub fn describe(e: &ServerFnError<shared::ApiError>) -> (Failure, String) {
    // `WrappedServerError` is soft-deprecated (server_fn 0.8.8) in favor of
    // authoring a wholly custom `FromServerFnError` type instead of
    // `ServerFnError<CustErr>` — but the generic remains fully supported
    // (`server_fn`'s own test suite asserts `ServerFnError: FromServerFnError`),
    // and matching this variant is the only way to read the typed `ApiError`
    // back out of it.
    #[allow(deprecated)]
    if let ServerFnError::WrappedServerError(api_err) = e {
        return (
            Failure::of_api_error(api_err),
            api_err.message().to_string(),
        );
    }
    let raw = match e {
        ServerFnError::ServerError(msg) => msg.clone(),
        other => other.to_string(),
    };
    let (failure, message) = classify(&raw);
    (failure, message.to_string())
}

/// Which *kind* of not-the-happy-path a surface is in, for the one badge that
/// says so. See the module doc: three claims, three token families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Nothing to show because nothing is left to do. The `success` family.
    Resolved,
    /// Some of this surface is missing and the rest of it still works. The
    /// `warning` family.
    Partial,
    /// What is on screen is real, but it does not answer the question currently
    /// being asked. The `info` family.
    Stale,
}

impl Tone {
    /// The token family. Deliberately total and deliberately dull — the point of
    /// routing every tone through here is that no arm picks a color.
    pub fn variant(self) -> BadgeVariant {
        match self {
            Tone::Resolved => BadgeVariant::Success,
            Tone::Partial => BadgeVariant::Warning,
            Tone::Stale => BadgeVariant::Info,
        }
    }

    /// The `data-tone` value — what a test and a bench section read.
    pub fn slug(self) -> &'static str {
        match self {
            Tone::Resolved => "resolved",
            Tone::Partial => "partial",
            Tone::Stale => "stale",
        }
    }
}

/// The badge that names a state arm's kind.
#[component]
pub fn StateBadge(tone: Tone, #[prop(into)] label: String) -> impl IntoView {
    view! {
        // `attr:`, not a `{..}` spread: the spread form mis-parses hyphenated
        // attribute names in this leptos version (the V1 vendoring convention).
        <Badge variant=tone.variant() size=BadgeSize::Sm attr:data-tone=tone.slug()>
            {label}
        </Badge>
    }
}

/// The one retry control. A `<button>` rather than a link: it refetches in
/// place, and a link would lose the scroll position and every unsaved control on
/// the page.
#[component]
pub fn RetryButton(on_retry: Callback<()>) -> impl IntoView {
    view! {
        <Button
            variant=ButtonVariant::Outline
            size=ButtonSize::Sm
            attr:data-testid="state-retry"
            on:click=move |_| on_retry.run(())
        >
            "Try again"
        </Button>
    }
}

/// The error banner every data surface renders, with the affordances the failure
/// actually warrants.
///
/// * `what` is the surface's own sentence — "Couldn't load your cards". It names
///   the *read*, not the page, so a partial failure doesn't read as a broken app.
/// * `retry` is rendered only for [`Failure::retryable`]. Passing it is a claim
///   that a refetch is possible, not that it is useful.
/// * `children` is the way out: a destination that does **not** depend on the
///   read that just failed. Always rendered when given, because the failure that
///   most needs it — a shared `?cursor=` gone stale — is the one with nothing for
///   the user to fix.
#[component]
pub fn ErrorNote(
    #[prop(into)] what: String,
    e: ServerFnError<shared::ApiError>,
    #[prop(into)] testid: String,
    #[prop(optional)] retry: Option<Callback<()>>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    let (failure, detail) = describe(&e);
    // Read untracked at build time, exactly as the catalog's sign-in prompts do:
    // the destination is where the reader *is*, and a tracked read here would
    // rebuild the banner on every navigation that already replaced it.
    let signin_href = (failure == Failure::Session).then(|| {
        let location = leptos_router::hooks::use_location();
        let path = location.pathname.get_untracked();
        let search = location.search.get_untracked();
        let here = if search.is_empty() {
            path
        } else {
            format!("{path}?{search}")
        };
        format!("/login?next={}", crate::catalog::encode_query_value(&here))
    });
    let message = match failure {
        // Not the page's failure, and the raw `unauthorized: invalid token` it
        // used to print reads as one.
        Failure::Session => "Your session has expired.".to_string(),
        // The detail here is a bare noun, so it is dropped rather than
        // concatenated — see `Failure::Missing`. "May" is exact: 404 answers both
        // "deleted" and "never yours", and the API conflates them on purpose.
        Failure::Missing => format!("{what} — it may have been deleted."),
        _ => format!("{what}: {detail}"),
    };
    let retry = retry.filter(|_| failure.retryable());
    // Whether the row below exists at all, decided here rather than by CSS: a
    // Leptos `None` still emits a hydration marker comment, so `:empty` would
    // never match and an affordance-less banner would carry a phantom gap.
    let affordances = retry.is_some() || signin_href.is_some() || children.is_some();

    view! {
        <div
            class="border-destructive/40 bg-destructive/10 rounded-md border px-3 py-2"
            data-testid=testid
            data-failure=failure.slug()
        >
            <p role="alert" class="text-destructive text-sm">
                {message}
            </p>
            // One row, and absent when the failure warrants nothing — an error
            // with no affordance is still better than a fabricated one.
            {affordances
                .then(|| {
                    view! {
                        <div class="mt-2 flex flex-wrap items-center gap-3">
                            {retry.map(|on_retry| view! { <RetryButton on_retry /> })}
                            {signin_href
                                .map(|href| {
                                    view! {
                                        <a
                                            href=href
                                            class="text-destructive text-sm font-medium underline"
                                            data-testid="state-signin"
                                        >
                                            "Sign in again"
                                        </a>
                                    }
                                })}
                            {children.map(|c| c())}
                        </div>
                    }
                })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_cursor_is_the_requests_fault_and_offers_no_retry() {
        // The concrete case: `?cursor=` from a shared or bookmarked link. The
        // server calls it a validation error, and retrying re-sends the same
        // cursor — so the arm must offer a way out instead.
        let (failure, message) = classify("validation: invalid cursor");
        assert_eq!(failure, Failure::Request);
        assert_eq!(message, "invalid cursor");
        assert!(!failure.retryable());
    }

    #[test]
    fn a_refused_op_is_also_the_requests_fault() {
        for raw in [
            "conflict: the Inbox cannot be moved",
            "forbidden: not yours",
        ] {
            let (failure, _) = classify(raw);
            assert_eq!(failure, Failure::Request, "{raw}");
            assert!(!failure.retryable(), "{raw}");
        }
    }

    #[test]
    fn a_dead_link_is_missing_and_its_detail_is_a_bare_noun() {
        // What `/my/collections/<deleted id>` actually produces. The detail is
        // the word "collection" — which is why this class does not concatenate
        // it, and why it is not lumped in with `Request`.
        let (failure, message) = classify("not found: collection");
        assert_eq!(failure, Failure::Missing);
        assert_eq!(message, "collection");
        assert!(!failure.retryable());
    }

    #[test]
    fn every_class_has_its_own_slug() {
        // The `data-failure` seam: two classes sharing a slug would let a test
        // pass while the arm offered the other one's affordances.
        let slugs = [
            Failure::Missing,
            Failure::Request,
            Failure::Session,
            Failure::Transport,
        ]
        .map(Failure::slug);
        assert_eq!(slugs, ["missing", "request", "session", "transport"]);
    }

    #[test]
    fn an_expired_session_is_its_own_arm() {
        // The string a human already mistook for a page bug.
        let (failure, message) = classify("unauthorized: invalid token");
        assert_eq!(failure, Failure::Session);
        assert_eq!(message, "invalid token");
        // A retry would 401 again; the fix is a sign-in.
        assert!(!failure.retryable());
    }

    #[test]
    fn upstream_and_anything_unrecognized_are_retryable() {
        // `upstream:` is the DB or the hosted API being unreachable — on the
        // native backend, an offline phone, which is the ordinary case.
        let (failure, message) = classify("upstream: database unavailable");
        assert_eq!(failure, Failure::Transport);
        assert_eq!(message, "database unavailable");
        assert!(failure.retryable());

        // No prefix at all: a transport-level `ServerFnError` (a dropped fetch,
        // a deserialization failure). Unrecognized must not become "your
        // request is wrong" — that would withhold the retry that fixes it.
        let (failure, message) = classify("error reaching server");
        assert_eq!(failure, Failure::Transport);
        assert_eq!(message, "error reaching server");
    }

    #[test]
    fn describe_reads_a_server_fn_error() {
        // A `ServerError(String)` carries no typed `ApiError` — this is the
        // string-fallback path, unchanged from before P6-083.
        let e = ServerFnError::<shared::ApiError>::ServerError("upstream: neon said no".into());
        assert_eq!(
            describe(&e),
            (Failure::Transport, "neon said no".to_string())
        );
    }

    /// The P6-083 path itself: a `WrappedServerError(ApiError)` — what
    /// `crate::api_err` now produces — is classified by matching the variant,
    /// with no `Display`-prefix parsing involved. Every variant is checked so
    /// a future edit to `Failure::of_api_error`'s match can't silently drop
    /// one into the wrong class.
    #[test]
    fn describe_classifies_a_typed_api_error_by_variant_not_by_string() {
        let cases = [
            (
                shared::ApiError::NotFound("collection".into()),
                Failure::Missing,
                "collection",
            ),
            (
                shared::ApiError::Unauthorized("invalid token".into()),
                Failure::Session,
                "invalid token",
            ),
            (
                shared::ApiError::Forbidden("not yours".into()),
                Failure::Request,
                "not yours",
            ),
            (
                shared::ApiError::Conflict("the Inbox cannot be moved".into()),
                Failure::Request,
                "the Inbox cannot be moved",
            ),
            (
                shared::ApiError::Validation("invalid cursor".into()),
                Failure::Request,
                "invalid cursor",
            ),
            (
                shared::ApiError::Upstream("neon unreachable".into()),
                Failure::Transport,
                "neon unreachable",
            ),
        ];
        for (api_err, want_failure, want_message) in cases {
            let e = ServerFnError::<shared::ApiError>::from(api_err.clone());
            assert_eq!(
                describe(&e),
                (want_failure, want_message.to_string()),
                "{api_err:?}"
            );
        }
    }

    #[test]
    fn each_tone_keeps_its_own_token_family() {
        // The mapping is the whole point of the type: "nothing left to do" must
        // never render in the destructive or warning family, and a partial page
        // must never render as success. (`==` rather than `assert_eq!`: the
        // vendored `BadgeVariant` derives no `Debug`, and adding one to a
        // registry file to please a test would be the tail wagging the dog.)
        assert!(Tone::Resolved.variant() == BadgeVariant::Success);
        assert!(Tone::Partial.variant() == BadgeVariant::Warning);
        assert!(Tone::Stale.variant() == BadgeVariant::Info);
        let slugs = [Tone::Resolved, Tone::Partial, Tone::Stale].map(Tone::slug);
        assert_eq!(slugs, ["resolved", "partial", "stale"]);
    }
}
