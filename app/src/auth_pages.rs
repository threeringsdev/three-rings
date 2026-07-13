//! Minimal `/login` + `/signup` screens (specs/auth.md step 6) in the
//! spike's dark visual language. Forms post to the account server fns via
//! `ActionForm`, so they degrade to plain form posts before hydration.

use leptos::form::ActionForm;
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_query_map};

use crate::account::{
    AuthOutcome, CurrentUser, GoogleSignIn, ResendVerification, SignIn, SignUp, VerifyEmail,
};

const CARD: &str =
    "bg-[#263343] rounded-xl shadow-2xl p-8 max-w-md w-full border border-[#3a4a5c] space-y-6";
const SCREEN: &str = "min-h-screen bg-[#1a2332] flex items-center justify-center p-4";
const INPUT: &str = "w-full rounded-lg bg-[#1a2332] border border-[#3a4a5c] px-4 py-3 text-white \
                     placeholder-[#8b9cb8] focus:outline-none focus:border-[#00d4aa]";
const BUTTON: &str = "w-full rounded-lg bg-[#00d4aa] px-6 py-3 text-[#1a2332] font-medium \
                      transition-all duration-200 hover:bg-[#00b894] active:scale-[0.98] \
                      disabled:opacity-50 disabled:cursor-not-allowed";
const BUTTON_GHOST: &str = "w-full rounded-lg border border-[#3a4a5c] px-6 py-3 text-white \
                            font-medium transition-all duration-200 hover:border-[#00d4aa] \
                            disabled:opacity-50";
const ERROR_TEXT: &str = "text-sm text-red-400";
const MUTED_TEXT: &str = "text-[#8b9cb8] text-sm";

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

#[component]
pub fn LoginPage() -> impl IntoView {
    let sign_in = ServerAction::<SignIn>::new();
    let google = ServerAction::<GoogleSignIn>::new();
    let navigate = use_navigate();
    let query = use_query_map();

    // None → credentials form; Some(email) → the OTP verification step.
    let (otp_email, set_otp_email) = signal(None::<String>);
    let (error, set_error) = signal(None::<String>);

    Effect::new(move |_| match sign_in.value().get() {
        Some(Ok(AuthOutcome::SignedIn(_))) => navigate("/", Default::default()),
        Some(Ok(AuthOutcome::VerificationRequired { email })) => {
            set_error.set(None);
            set_otp_email.set(Some(email));
        }
        Some(Ok(AuthOutcome::Failed { message })) => set_error.set(Some(message)),
        Some(Err(_)) => set_error.set(Some("Something went wrong — try again.".into())),
        None => {}
    });

    Effect::new(move |_| {
        if let Some(Ok(url)) = google.value().get() {
            redirect_browser(&url);
        }
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
                <div class=CARD>
                    <h1 class="text-2xl font-medium text-white">"Sign in"</h1>
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
                        <button class=BUTTON type="submit" disabled=move || sign_in.pending().get()>
                            {move || if sign_in.pending().get() { "Signing in…" } else { "Sign in" }}
                        </button>
                    </ActionForm>
                    <button
                        class=BUTTON_GHOST
                        on:click=move |_| {
                            google.dispatch(GoogleSignIn {});
                        }
                        disabled=move || google.pending().get()
                    >
                        "Continue with Google"
                    </button>
                    <Show when=move || error.get().is_some() || google_error().is_some()>
                        <p class=ERROR_TEXT>
                            {move || error.get().or_else(|| google_error().map(str::to_string))}
                        </p>
                    </Show>
                    <p class=MUTED_TEXT>
                        "No account? " <a class="underline text-white" href="/signup">"Sign up"</a>
                    </p>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn SignupPage() -> impl IntoView {
    let sign_up = ServerAction::<SignUp>::new();
    let navigate = use_navigate();

    let (otp_email, set_otp_email) = signal(None::<String>);
    let (error, set_error) = signal(None::<String>);

    Effect::new(move |_| match sign_up.value().get() {
        Some(Ok(AuthOutcome::SignedIn(_))) => navigate("/", Default::default()),
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
                    <h1 class="text-2xl font-medium text-white">"Create account"</h1>
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
                    <Show when=move || error.get().is_some()>
                        <p class=ERROR_TEXT>{move || error.get()}</p>
                    </Show>
                    <p class=MUTED_TEXT>
                        "Already have an account? "
                        <a class="underline text-white" href="/login">"Sign in"</a>
                    </p>
                </div>
            </Show>
        </div>
    }
}

/// The "enter the code we emailed you" step, shared by login and signup.
#[component]
fn OtpCard(email: String) -> impl IntoView {
    let verify = ServerAction::<VerifyEmail>::new();
    let resend = ServerAction::<ResendVerification>::new();
    let navigate = use_navigate();
    let (error, set_error) = signal(None::<String>);

    Effect::new(move |_| match verify.value().get() {
        Some(Ok(AuthOutcome::SignedIn(_))) => navigate("/", Default::default()),
        Some(Ok(AuthOutcome::Failed { message })) => set_error.set(Some(message)),
        Some(Ok(AuthOutcome::VerificationRequired { .. })) | None => {}
        Some(Err(_)) => set_error.set(Some("Something went wrong — try again.".into())),
    });

    let resent = move || matches!(resend.value().get(), Some(Ok(())));
    let email_display = email.clone();
    let email_field = email.clone();

    view! {
        <div class=CARD>
            <h1 class="text-2xl font-medium text-white">"Check your email"</h1>
            <p class=MUTED_TEXT>
                "We sent a verification code to " <span class="text-white">{email_display}</span>
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
        </div>
    }
}

/// Compact signed-in indicator for the home page footer: the current user's
/// email with sign-out, or sign-in/sign-up links.
#[component]
pub fn AuthStatus() -> impl IntoView {
    let user = Resource::new(|| (), |_| crate::account::fetch_current_user());
    let sign_out = ServerAction::<crate::account::SignOut>::new();

    Effect::new(move |_| {
        if matches!(sign_out.value().get(), Some(Ok(()))) {
            user.refetch();
        }
    });

    view! {
        <div class="text-[#8b9cb8] text-xs">
            <Suspense fallback=|| ()>
                {move || Suspend::new(async move {
                    match user.await {
                        Ok(Some(CurrentUser { email, name, .. })) => {
                            let who = email.or(name).unwrap_or_else(|| "you".into());
                            view! {
                                <span>
                                    "Signed in as " <span class="text-white">{who}</span> " · "
                                    <button
                                        class="underline"
                                        on:click=move |_| {
                                            sign_out.dispatch(crate::account::SignOut {});
                                        }
                                    >
                                        "Sign out"
                                    </button>
                                </span>
                            }
                                .into_any()
                        }
                        _ => view! {
                            <span>
                                <a class="underline" href="/login">"Sign in"</a> " · "
                                <a class="underline" href="/signup">"Sign up"</a>
                            </span>
                        }
                            .into_any(),
                    }
                })}
            </Suspense>
        </div>
    }
}
