use leptos::prelude::*;
use leptos_router::components::A;

use crate::browser::navigate_to;
use crate::server_fns::{get_registration_status, Register, RegistrationStatus};

/// A public, top-level page (outside `DashboardLayout`, which gates
/// everything behind login) — an unauthenticated visitor needs to reach
/// this directly, including the very first person to ever open the
/// instance.
#[component]
pub fn RegisterPage() -> impl IntoView {
    let status = Resource::new(|| (), |_| get_registration_status());

    view! {
        <div class="login-screen">
            <div class="login-card">
                <span class="wordmark">"ravyn"</span>
                <Suspense fallback=|| view! { <p class="login-tagline">"loading..."</p> }>
                    {move || {
                        status
                            .get()
                            .map(|result| match result {
                                Ok(status) => view! { <RegisterForm status/> }.into_any(),
                                Err(_) => {
                                    view! {
                                        <p class="form-error">
                                            "couldn't check this instance's registration status."
                                        </p>
                                    }
                                        .into_any()
                                }
                            })
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[component]
fn RegisterForm(status: RegistrationStatus) -> impl IntoView {
    if status.mode == "closed" && !status.setup_required {
        return view! {
            <p class="login-tagline">"registration is closed on this instance."</p>
            <A href="/">"back to login"</A>
        }
        .into_any();
    }

    let register_action = ServerAction::<Register>::new();
    let (username, set_username) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (invite_token, set_invite_token) = signal(String::new());
    let needs_invite = status.mode == "invite" && !status.setup_required;
    let setup_required = status.setup_required;

    Effect::new(move |_| {
        if register_action
            .value()
            .get()
            .is_some_and(|result| result.is_ok())
        {
            navigate_to("/");
        }
    });

    view! {
        <p class="login-tagline">
            {if setup_required {
                "no admin account yet — set one up to finish installing ravyn."
            } else {
                "create an account"
            }}
        </p>
        <form on:submit=move |ev| {
            ev.prevent_default();
            register_action
                .dispatch(Register {
                    username: username.get(),
                    password: password.get(),
                    invite_token: needs_invite.then(|| invite_token.get()),
                });
        }>
            <div class="field">
                <label for="reg-username">"username"</label>
                <input
                    id="reg-username"
                    type="text"
                    autocomplete="username"
                    on:input=move |ev| set_username.set(event_target_value(&ev))
                />
            </div>
            <div class="field">
                <label for="reg-password">"password"</label>
                <input
                    id="reg-password"
                    type="password"
                    autocomplete="new-password"
                    on:input=move |ev| set_password.set(event_target_value(&ev))
                />
            </div>
            {needs_invite
                .then(|| {
                    view! {
                        <div class="field">
                            <label for="reg-invite">"invite code"</label>
                            <input
                                id="reg-invite"
                                type="text"
                                on:input=move |ev| set_invite_token.set(event_target_value(&ev))
                            />
                        </div>
                    }
                })}
            <button type="submit" class="btn btn-primary btn-block">
                {if setup_required { "create admin account" } else { "create account" }}
            </button>
            {move || {
                register_action
                    .value()
                    .get()
                    .and_then(|result| result.err())
                    .map(|err| view! { <p class="form-error">{err.to_string()}</p> })
            }}
        </form>
        {(!setup_required)
            .then(|| {
                view! {
                    <p class="login-tagline">
                        <A href="/">"back to login"</A>
                    </p>
                }
            })}
    }
    .into_any()
}
