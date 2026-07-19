//! Minimal `/login` + `/signup` screens (specs/auth.md step 6) in the
//! spike's dark visual language. Forms post to the account server fns via
//! `ActionForm`, so they degrade to plain form posts before hydration.

use leptos::form::ActionForm;
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use crate::account::{
    AuthOutcome, GoogleSignIn, RequestPasswordReset, ResendVerification, ResetPassword, SignIn,
    SignUp, VerifyEmail,
};

const CARD: &str =
    "bg-card text-card-foreground rounded-xl shadow-2xl p-8 max-w-md w-full border space-y-6";
const SCREEN: &str = "min-h-screen bg-background flex items-center justify-center p-4";
const INPUT: &str = "w-full rounded-lg bg-background border border-input px-4 py-3 \
                     placeholder-muted-foreground focus:outline-none focus:border-ring";
const BUTTON: &str = "w-full rounded-lg bg-primary px-6 py-3 text-primary-foreground font-medium \
                      transition-all duration-200 hover:bg-primary/90 active:scale-[0.98] \
                      disabled:opacity-50 disabled:cursor-not-allowed";
const BUTTON_GHOST: &str = "w-full rounded-lg border px-6 py-3 \
                            font-medium transition-all duration-200 hover:border-ring \
                            disabled:opacity-50";
const ERROR_TEXT: &str = "text-sm text-destructive";
const MUTED_TEXT: &str = "text-muted-foreground text-sm";

/// Navigate the browser itself (full page load) — used to hand the window to
/// the Google flow, which leaves our origin.
fn redirect_browser(url: &str) {
    #[cfg(feature = "hydrate")]
    {
        if let Some(w) = web_sys::window() {
            let _ = w.location().set_href(url);
        }
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = url;
    }
}

/// Where to land after a successful sign-in: the `next` query param when it
/// is a same-origin path (the `/my/*` auth guard appends it), else `/` —
/// whose redirect sends authed users on to `/my`. Anything not starting with
/// a single `/` is ignored (open-redirect guard) — including `/\`, which
/// browsers normalize to the protocol-relative `//`.
fn post_auth_destination(query: &leptos_router::params::ParamsMap) -> String {
    match query.get("next") {
        Some(next)
            if next.starts_with('/') && !next.starts_with("//") && !next.starts_with("/\\") =>
        {
            next
        }
        _ => "/".into(),
    }
}

/// Inside a Tauri shell, open the URL in the *system browser* — Google
/// refuses OAuth in embedded webviews. Calls the app's `open_url` command via
/// `window.__TAURI__.core.invoke`; `on_reject` fires with a message if the
/// shell rejects it (so failures surface in the UI instead of a silent
/// "waiting" state). Returns false when not running under Tauri (plain web).
fn tauri_open_url(url: &str, on_reject: impl Fn(String) + 'static) -> bool {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::{closure::Closure, JsCast, JsValue};
        let Some(w) = web_sys::window() else {
            return false;
        };
        let Ok(tauri) = js_sys::Reflect::get(&w, &JsValue::from_str("__TAURI__")) else {
            return false;
        };
        if tauri.is_undefined() || tauri.is_null() {
            return false;
        }
        let Ok(core) = js_sys::Reflect::get(&tauri, &JsValue::from_str("core")) else {
            return false;
        };
        let Ok(invoke) = js_sys::Reflect::get(&core, &JsValue::from_str("invoke")) else {
            return false;
        };
        let Ok(invoke) = invoke.dyn_into::<js_sys::Function>() else {
            return false;
        };
        let args = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&args, &JsValue::from_str("url"), &JsValue::from_str(url));
        let Ok(result) = invoke.call2(&core, &JsValue::from_str("open_url"), &args) else {
            return false;
        };
        let Ok(promise) = result.dyn_into::<js_sys::Promise>() else {
            return false;
        };
        let catch = Closure::once(move |err: JsValue| {
            on_reject(
                err.as_string()
                    .unwrap_or_else(|| "could not open the browser".into()),
            );
        });
        let _ = promise.catch(&catch);
        catch.forget();
        true
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (url, on_reject);
        false
    }
}

/// Start a browser interval; returns its id. Hydrate-only (client).
fn set_poll_interval(f: impl FnMut() + 'static, ms: i32) -> Option<i32> {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::closure::Closure;
        use wasm_bindgen::JsCast;
        let cb = Closure::wrap(Box::new(f) as Box<dyn FnMut()>);
        let id = web_sys::window()?
            .set_interval_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), ms)
            .ok()?;
        // Leaked deliberately: one short-lived closure per sign-in attempt,
        // cleared via clear_poll_interval when the flow resolves.
        cb.forget();
        Some(id)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (f, ms);
        None
    }
}

fn clear_poll_interval(id: i32) {
    #[cfg(feature = "hydrate")]
    {
        if let Some(w) = web_sys::window() {
            w.clear_interval_with_handle(id);
        }
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = id;
    }
}

/// "Continue with Google", shared by the login and signup cards.
///
/// Web: navigates this window into the hosted flow (`/auth/callback` lands us
/// back signed in). Tauri: opens the flow in the system browser and polls
/// `current_user` until the embedded server has parked the session
/// (specs/auth.md → desktop Google plan), then heads home.
#[component]
fn GoogleButton(set_error: WriteSignal<Option<String>>) -> impl IntoView {
    let google = ServerAction::<GoogleSignIn>::new();
    let poll = Action::new(|_: &()| crate::account::fetch_current_user());
    let (waiting, set_waiting) = signal(false);
    let interval_id: StoredValue<Option<i32>> = StoredValue::new(None);
    let attempts: StoredValue<i32> = StoredValue::new(0);

    let stop_polling = move || {
        if let Some(id) = interval_id.try_get_value().flatten() {
            clear_poll_interval(id);
            interval_id.try_set_value(None);
        }
    };

    Effect::new(move |_| match google.value().get() {
        Some(Ok(url)) => {
            let on_reject = move |message: String| {
                stop_polling();
                set_waiting.set(false);
                set_error.set(Some(format!("Couldn't open the browser: {message}")));
            };
            if tauri_open_url(&url, on_reject) {
                set_waiting.set(true);
                attempts.set_value(0);
                let id = set_poll_interval(
                    move || {
                        attempts.try_update_value(|n| *n += 1);
                        if attempts.try_get_value().unwrap_or(i32::MAX) > 90 {
                            stop_polling();
                        } else {
                            poll.dispatch(());
                        }
                    },
                    2000,
                );
                interval_id.set_value(id);
            } else {
                redirect_browser(&url);
            }
        }
        // Transport/config failure — on the web this shouldn't happen; keep
        // the message honest either way.
        Some(Err(_)) => set_error.set(Some(
            "Google sign-in isn't available right now — use email and password.".into(),
        )),
        None => {}
    });

    Effect::new(move |_| {
        if let Some(Ok(Some(_user))) = poll.value().get() {
            stop_polling();
            // Full-page load for the same reason as the password flows: SSR
            // re-dispatches the now-authed session (302 → /my).
            redirect_browser("/");
        }
    });

    on_cleanup(stop_polling);

    view! {
        <button
            class=BUTTON_GHOST
            on:click=move |_| {
                google.dispatch(GoogleSignIn {});
            }
            disabled=move || google.pending().get() || waiting.get()
        >
            {move || {
                if waiting.get() { "Waiting for Google…" } else { "Continue with Google" }
            }}
        </button>
    }
}

/// Muted escape hatch back to the home page — native apps have no browser
/// back button.
#[component]
fn BackHome() -> impl IntoView {
    view! {
        <p class=MUTED_TEXT>
            <a class="underline" href="/">
                "← Back to home"
            </a>
        </p>
    }
}

#[component]
pub fn LoginPage() -> impl IntoView {
    let sign_in = ServerAction::<SignIn>::new();
    let query = use_query_map();

    // None → credentials form; Some(email) → the OTP verification step.
    let (otp_email, set_otp_email) = signal(None::<String>);
    // true → the password-reset card replaces the credentials form.
    let (reset_mode, set_reset_mode) = signal(false);
    let (error, set_error) = signal(None::<String>);

    Effect::new(move |_| match sign_in.value().get() {
        // Full-page load, not SPA navigation: the shell's shared current-user
        // resource was fetched while anonymous, so client-side routing would
        // dispatch on the stale session. A document load re-runs SSR with the
        // fresh cookies and the server 302s to the right mode.
        Some(Ok(AuthOutcome::SignedIn(_))) => {
            redirect_browser(&post_auth_destination(&query.get_untracked()))
        }
        Some(Ok(AuthOutcome::VerificationRequired { email })) => {
            set_error.set(None);
            set_otp_email.set(Some(email));
        }
        Some(Ok(AuthOutcome::Failed { message })) => set_error.set(Some(message)),
        Some(Err(_)) => set_error.set(Some("Something went wrong — try again.".into())),
        None => {}
    });

    let google_error = move || {
        (query.read().get("error").as_deref() == Some("google"))
            .then_some("Google sign-in didn't complete — try again.")
    };

    view! {
        <div class=SCREEN>
            <Show
                when=move || otp_email.get().is_none()
                fallback=move || {
                    view! { <OtpCard email=otp_email.get().unwrap_or_default() /> }
                }
            >
                <Show
                    when=move || !reset_mode.get()
                    fallback=move || view! { <ResetCard set_reset_mode=set_reset_mode /> }
                >
                    <div class=CARD>
                        <h1 class="text-2xl font-medium">"Sign in"</h1>
                        <ActionForm action=sign_in attr:class="space-y-4">
                            <input
                                class=INPUT
                                type="email"
                                name="email"
                                placeholder="Email"
                                required
                            />
                            <input
                                class=INPUT
                                type="password"
                                name="password"
                                placeholder="Password"
                                required
                            />
                            <button
                                class=BUTTON
                                type="submit"
                                disabled=move || sign_in.pending().get()
                            >
                                {move || {
                                    if sign_in.pending().get() { "Signing in…" } else { "Sign in" }
                                }}
                            </button>
                        </ActionForm>
                        <GoogleButton set_error=set_error />
                        <Show when=move || error.get().is_some() || google_error().is_some()>
                            <p class=ERROR_TEXT>
                                {move || error.get().or_else(|| google_error().map(str::to_string))}
                            </p>
                        </Show>
                        <p class=MUTED_TEXT>
                            <button
                                class="underline"
                                on:click=move |_| set_reset_mode.set(true)
                            >
                                "Forgot password?"
                            </button>
                        </p>
                        <p class=MUTED_TEXT>
                            "No account? "
                            <a class="underline" href="/signup">"Sign up"</a>
                        </p>
                        <BackHome />
                    </div>
                </Show>
            </Show>
        </div>
    }
}

#[component]
pub fn SignupPage() -> impl IntoView {
    let sign_up = ServerAction::<SignUp>::new();
    let query = use_query_map();

    let (otp_email, set_otp_email) = signal(None::<String>);
    let (error, set_error) = signal(None::<String>);

    Effect::new(move |_| match sign_up.value().get() {
        // Full-page load, not SPA navigation: the shell's shared current-user
        // resource was fetched while anonymous, so client-side routing would
        // dispatch on the stale session. A document load re-runs SSR with the
        // fresh cookies and the server 302s to the right mode.
        Some(Ok(AuthOutcome::SignedIn(_))) => {
            redirect_browser(&post_auth_destination(&query.get_untracked()))
        }
        Some(Ok(AuthOutcome::VerificationRequired { email })) => {
            set_error.set(None);
            set_otp_email.set(Some(email));
        }
        Some(Ok(AuthOutcome::Failed { message })) => set_error.set(Some(message)),
        Some(Err(_)) => set_error.set(Some("Something went wrong — try again.".into())),
        None => {}
    });

    view! {
        <div class=SCREEN>
            <Show
                when=move || otp_email.get().is_none()
                fallback=move || {
                    view! { <OtpCard email=otp_email.get().unwrap_or_default() /> }
                }
            >
                <div class=CARD>
                    <h1 class="text-2xl font-medium">"Create account"</h1>
                    <ActionForm action=sign_up attr:class="space-y-4">
                        <input class=INPUT type="text" name="name" placeholder="Name" required />
                        <input
                            class=INPUT
                            type="email"
                            name="email"
                            placeholder="Email"
                            required
                        />
                        <input
                            class=INPUT
                            type="password"
                            name="password"
                            placeholder="Password (8+ characters)"
                            required
                            minlength="8"
                        />
                        <button class=BUTTON type="submit" disabled=move || sign_up.pending().get()>
                            {move || {
                                if sign_up.pending().get() { "Creating…" } else { "Create account" }
                            }}
                        </button>
                    </ActionForm>
                    <GoogleButton set_error=set_error />
                    <Show when=move || error.get().is_some()>
                        <p class=ERROR_TEXT>{move || error.get()}</p>
                    </Show>
                    <p class=MUTED_TEXT>
                        "Already have an account? "
                        <a class="underline" href="/login">"Sign in"</a>
                    </p>
                    <BackHome />
                </div>
            </Show>
        </div>
    }
}

/// Two-step password reset: request the emailed code, then set the new
/// password with it. Doubles as **"add a password to a Google-first
/// account"** — the upstream creates the credential account when none
/// exists (specs/auth.md). On success the server fn signs the user in with
/// the fresh credentials, so this lands on the home page like any sign-in.
#[component]
fn ResetCard(set_reset_mode: WriteSignal<bool>) -> impl IntoView {
    let request = ServerAction::<RequestPasswordReset>::new();
    let reset = ServerAction::<ResetPassword>::new();
    let query = use_query_map();

    // Mirrors the email input so the OTP step knows the address once the
    // request action resolves (the form itself posts via ActionForm).
    let (email_draft, set_email_draft) = signal(String::new());
    let (sent_to, set_sent_to) = signal(None::<String>);
    let (error, set_error) = signal(None::<String>);

    Effect::new(move |_| match request.value().get() {
        Some(Ok(())) => {
            set_error.set(None);
            set_sent_to.set(Some(email_draft.get_untracked()));
        }
        Some(Err(_)) => set_error.set(Some("Couldn't send the code — try again.".into())),
        None => {}
    });

    Effect::new(move |_| match reset.value().get() {
        // Full-page load, not SPA navigation: the shell's shared current-user
        // resource was fetched while anonymous, so client-side routing would
        // dispatch on the stale session. A document load re-runs SSR with the
        // fresh cookies and the server 302s to the right mode.
        Some(Ok(AuthOutcome::SignedIn(_))) => {
            redirect_browser(&post_auth_destination(&query.get_untracked()))
        }
        Some(Ok(AuthOutcome::Failed { message })) => set_error.set(Some(message)),
        // Reset marks the email verified upstream, so this shouldn't fire;
        // keep the arm honest in case that behavior shifts.
        Some(Ok(AuthOutcome::VerificationRequired { .. })) => {
            set_error.set(Some("Verify your email first, then sign in.".into()))
        }
        Some(Err(_)) => set_error.set(Some("Something went wrong — try again.".into())),
        None => {}
    });

    view! {
        <div class=CARD>
            <h1 class="text-2xl font-medium">"Reset password"</h1>
            <Show
                when=move || sent_to.get().is_none()
                fallback=move || {
                    let email = sent_to.get().unwrap_or_default();
                    let email_display = email.clone();
                    let email_field = email.clone();
                    let email_resend = email.clone();
                    view! {
                        <p class=MUTED_TEXT>
                            "We sent a reset code to "
                            <span class="text-foreground">{email_display}</span>
                        </p>
                        <ActionForm action=reset attr:class="space-y-4">
                            <input type="hidden" name="email" value=email_field />
                            <input
                                class=INPUT
                                type="text"
                                name="otp"
                                placeholder="Reset code"
                                inputmode="numeric"
                                autocomplete="one-time-code"
                                required
                            />
                            <input
                                class=INPUT
                                type="password"
                                name="password"
                                placeholder="New password (8+ characters)"
                                required
                                minlength="8"
                            />
                            <button
                                class=BUTTON
                                type="submit"
                                disabled=move || reset.pending().get()
                            >
                                {move || {
                                    if reset.pending().get() { "Saving…" } else { "Set new password" }
                                }}
                            </button>
                        </ActionForm>
                        <button
                            class=BUTTON_GHOST
                            on:click=move |_| {
                                request
                                    .dispatch(RequestPasswordReset {
                                        email: email_resend.clone(),
                                    });
                            }
                            disabled=move || request.pending().get()
                        >
                            "Re-send code"
                        </button>
                    }
                }
            >
                <p class=MUTED_TEXT>
                    "Enter your account email and we'll send a reset code. This also \
                     works for adding a password to an account created with Google."
                </p>
                <ActionForm action=request attr:class="space-y-4">
                    <input
                        class=INPUT
                        type="email"
                        name="email"
                        placeholder="Email"
                        required
                        on:input=move |ev| set_email_draft.set(event_target_value(&ev))
                    />
                    <button class=BUTTON type="submit" disabled=move || request.pending().get()>
                        {move || {
                            if request.pending().get() { "Sending…" } else { "Email me a reset code" }
                        }}
                    </button>
                </ActionForm>
            </Show>
            <Show when=move || error.get().is_some()>
                <p class=ERROR_TEXT>{move || error.get()}</p>
            </Show>
            <p class=MUTED_TEXT>
                <button class="underline" on:click=move |_| set_reset_mode.set(false)>
                    "← Back to sign in"
                </button>
            </p>
        </div>
    }
}

/// The "enter the code we emailed you" step, shared by login and signup.
#[component]
fn OtpCard(email: String) -> impl IntoView {
    let verify = ServerAction::<VerifyEmail>::new();
    let resend = ServerAction::<ResendVerification>::new();
    let query = use_query_map();
    let (error, set_error) = signal(None::<String>);

    Effect::new(move |_| match verify.value().get() {
        // Full-page load, not SPA navigation: the shell's shared current-user
        // resource was fetched while anonymous, so client-side routing would
        // dispatch on the stale session. A document load re-runs SSR with the
        // fresh cookies and the server 302s to the right mode.
        Some(Ok(AuthOutcome::SignedIn(_))) => {
            redirect_browser(&post_auth_destination(&query.get_untracked()))
        }
        Some(Ok(AuthOutcome::Failed { message })) => set_error.set(Some(message)),
        Some(Ok(AuthOutcome::VerificationRequired { .. })) | None => {}
        Some(Err(_)) => set_error.set(Some("Something went wrong — try again.".into())),
    });

    let resent = move || matches!(resend.value().get(), Some(Ok(())));
    let email_display = email.clone();
    let email_field = email.clone();

    view! {
        <div class=CARD>
            <h1 class="text-2xl font-medium">"Check your email"</h1>
            <p class=MUTED_TEXT>
                "We sent a verification code to " <span class="text-foreground">{email_display}</span>
            </p>
            <ActionForm action=verify attr:class="space-y-4">
                <input type="hidden" name="email" value=email_field />
                <input
                    class=INPUT
                    type="text"
                    name="otp"
                    placeholder="Verification code"
                    inputmode="numeric"
                    autocomplete="one-time-code"
                    required
                />
                <button class=BUTTON type="submit" disabled=move || verify.pending().get()>
                    {move || if verify.pending().get() { "Verifying…" } else { "Verify" }}
                </button>
            </ActionForm>
            <button
                class=BUTTON_GHOST
                on:click={
                    let email = email.clone();
                    move |_| {
                        resend.dispatch(ResendVerification { email: email.clone() });
                    }
                }
                disabled=move || resend.pending().get()
            >
                {move || if resent() { "Code re-sent" } else { "Re-send code" }}
            </button>
            <Show when=move || error.get().is_some()>
                <p class=ERROR_TEXT>{move || error.get()}</p>
            </Show>
            <BackHome />
        </div>
    }
}
