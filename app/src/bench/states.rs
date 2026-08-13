//! Bench section for the **state arms** (`app/src/components/states.rs`) — the
//! shared error banner in each of its four failure classes, the retry control,
//! and the three tone badges.
//!
//! It is here for the reason the my-root list is: *these arms are the hardest
//! things in the app to look at.* Every one of them needs a read to fail, and
//! several need it to fail in a *particular* way — a stale `?cursor=`, an expired
//! session, an unreachable backend — while sitting on a page that is otherwise
//! healthy. On the Android emulator it is worse than hard: the dev proxy strips
//! Cookie headers, so every authed surface these banners live on redirects to
//! `/login?next=…` and cannot be reached at all. So the bench is where the arms
//! get seen, on-device included, exactly as the set picker's loading arm was
//! unreachable until its bench section made it reachable.
//!
//! What the four columns are asserting, and why it is worth a page: **the
//! affordances differ by failure class, on purpose**. A retry over a deleted id
//! or a stale cursor re-sends the same doomed request; a retry over a dropped
//! connection is the whole fix; an expired session is not the page's failure at
//! all. Rendering the four side by side is the cheapest way to see that the
//! banner is not offering one uniform gesture and calling it help.

use leptos::prelude::*;
use leptos::server_fn::ServerFnError;

use crate::components::states::{ErrorNote, RetryButton, StateBadge, Tone};

/// A synthetic wire error for the bench columns below. Deliberately the
/// **string-fallback** shape (`ServerError`, not `WrappedServerError`) so the
/// bench keeps exercising [`crate::components::states::classify`]'s
/// `Display`-prefix table over literal text — a real `crate::api_err` read
/// would arrive typed (P6-083), but this page has no live backend to fetch
/// one from, only this string standing in for the four wire shapes.
fn err(raw: &str) -> ServerFnError<shared::ApiError> {
    ServerFnError::ServerError(raw.to_string())
}

pub fn demo() -> AnyView {
    let retries = RwSignal::new(0u32);
    let bump = Callback::new(move |()| retries.update(|n| *n += 1));

    view! {
        <div class="space-y-6">
            <div class="space-y-2">
                <p class="text-muted-foreground text-sm">
                    "The three tones. Each is a different claim about why a surface is not showing its happy path — good news, partially missing, not current — and each maps to exactly one token family so no arm picks a color."
                </p>
                <div class="flex flex-wrap items-center gap-3">
                    <span class="flex items-center gap-1.5 text-sm">
                        <StateBadge tone=Tone::Resolved label="All set" />
                        <code class="text-muted-foreground text-xs">"success"</code>
                    </span>
                    <span class="flex items-center gap-1.5 text-sm">
                        <StateBadge tone=Tone::Partial label="Partial" />
                        <code class="text-muted-foreground text-xs">"warning"</code>
                    </span>
                    <span class="flex items-center gap-1.5 text-sm">
                        <StateBadge tone=Tone::Stale label="Previous results" />
                        <code class="text-muted-foreground text-xs">"info"</code>
                    </span>
                </div>
            </div>

            <div class="space-y-2">
                <p class="text-muted-foreground text-sm">
                    "The shared error banner, once per failure class. Note what each one offers: the two request-level failures get the way out and "
                    <em>"no"</em>
                    " retry, the transport failure gets the retry, and the expired session gets neither — it gets a sign-in."
                </p>
                <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                    <div class="space-y-1.5">
                        <p class="text-muted-foreground text-xs">
                            <code>"not found:"</code>
                            " — a link to a deleted collection. The detail is the bare noun "
                            <code>"collection"</code>", which is why it is not appended."
                        </p>
                        <ErrorNote
                            what="Couldn't load this collection"
                            e=err("not found: collection")
                            testid="bench-error-missing"
                            retry=bump
                        >
                            <a
                                href="#states"
                                class="text-destructive text-sm font-medium underline"
                                data-testid="bench-error-away"
                            >
                                "My cards"
                            </a>
                        </ErrorNote>
                    </div>
                    <div class="space-y-1.5">
                        <p class="text-muted-foreground text-xs">
                            <code>"validation:"</code>
                            " — a shared "<code>"?cursor="</code>" gone stale"
                        </p>
                        <ErrorNote
                            what="Couldn't load your cards"
                            e=err("validation: invalid cursor")
                            testid="bench-error-request"
                            retry=bump
                        >
                            <a
                                href="#states"
                                class="text-destructive text-sm font-medium underline"
                                data-testid="bench-error-home"
                            >
                                "← Back to the start"
                            </a>
                        </ErrorNote>
                    </div>
                    <div class="space-y-1.5">
                        <p class="text-muted-foreground text-xs">
                            <code>"upstream:"</code>" — an offline phone, the native backend's ordinary case"
                        </p>
                        <ErrorNote
                            what="Couldn't load your cards"
                            e=err("upstream: could not reach the server")
                            testid="bench-error-transport"
                            retry=bump
                        />
                    </div>
                    <div class="space-y-1.5">
                        <p class="text-muted-foreground text-xs">
                            <code>"unauthorized:"</code>" — the session, not the page"
                        </p>
                        <ErrorNote
                            what="Couldn't load your cards"
                            e=err("unauthorized: invalid token")
                            testid="bench-error-session"
                            retry=bump
                        />
                    </div>
                </div>
                // The retry has to be provably live, or the two banners above are
                // a screenshot: a count that moves is what says the callback is
                // wired and the tap target is real (which is what the Android
                // probe drives).
                <p class="text-muted-foreground text-sm">
                    "Retries fired: "
                    <span class="text-foreground font-medium tabular-nums" data-testid="bench-retries">
                        {move || retries.get()}
                    </span>
                </p>
            </div>

            <div class="space-y-2">
                <p class="text-muted-foreground text-sm">
                    "The retry control on its own — what "<code>"/cards/:id"</code>
                    "'s failed arm and the sidebar's failed tree both reach for."
                </p>
                <RetryButton on_retry=bump />
            </div>
        </div>
    }
    .into_any()
}
