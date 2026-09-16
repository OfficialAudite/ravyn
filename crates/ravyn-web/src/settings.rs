use leptos::prelude::*;

use crate::format::format_date;
use crate::icons::TrashIcon;
use crate::server_fns::{
    get_storage_info, list_api_tokens, me, ApiTokenInfo, CreateApiToken, DeleteApiToken,
};

#[component]
pub fn SettingsPage() -> impl IntoView {
    view! {
        <div class="section-head">
            <h2>"settings"</h2>
        </div>
        <AccountSection/>
        <ApiTokensSection/>
        <StorageSection/>
    }
}

#[component]
fn AccountSection() -> impl IntoView {
    let account = Resource::new(|| (), |_| me());

    view! {
        <div class="settings-section">
            <h3>"account"</h3>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    account
                        .get()
                        .map(|result| match result {
                            Ok(info) => {
                                view! {
                                    <p class="settings-row">
                                        "signed in as " <strong>{info.username}</strong>
                                    </p>
                                }
                                    .into_any()
                            }
                            Err(_) => {
                                view! { <p class="form-error">"failed to load account info"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
            <p class="settings-hint">
                "multiple users aren't supported yet — this instance has a single owner account."
            </p>
        </div>
    }
}

#[component]
fn ApiTokensSection() -> impl IntoView {
    let create_action = ServerAction::<CreateApiToken>::new();
    let delete_action = ServerAction::<DeleteApiToken>::new();
    let (name, set_name) = signal(String::new());

    let tokens = Resource::new(
        move || (create_action.version().get(), delete_action.version().get()),
        |_| list_api_tokens(),
    );

    view! {
        <div class="settings-section">
            <h3>"api tokens"</h3>
            <p class="settings-hint">
                "use a token for ShareX or any other uploader, as a Bearer token."
            </p>

            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    tokens
                        .get()
                        .map(|result| match result {
                            Ok(tokens) if tokens.is_empty() => {
                                view! { <p class="settings-hint">"no tokens yet."</p> }.into_any()
                            }
                            Ok(tokens) => {
                                view! {
                                    <ul class="token-list">
                                        {tokens
                                            .into_iter()
                                            .map(|token| view! { <TokenRow token delete_action /> })
                                            .collect_view()}
                                    </ul>
                                }
                                    .into_any()
                            }
                            Err(_) => {
                                view! { <p class="form-error">"failed to load tokens"</p> }.into_any()
                            }
                        })
                }}
            </Suspense>

            <form
                class="token-form"
                on:submit=move |ev| {
                    ev.prevent_default();
                    create_action.dispatch(CreateApiToken { name: name.get() });
                    set_name.set(String::new());
                }
            >
                <input
                    type="text"
                    placeholder="token name, e.g. ShareX"
                    prop:value=move || name.get()
                    on:input=move |ev| set_name.set(event_target_value(&ev))
                />
                <button type="submit" class="btn btn-primary">
                    "create token"
                </button>
            </form>
            {move || {
                create_action
                    .value()
                    .get()
                    .map(|result| match result {
                        Ok(token) => {
                            view! {
                                <p class="token-result">
                                    "copy it now — it won't be shown again: " {token}
                                </p>
                            }
                                .into_any()
                        }
                        Err(err) => view! { <p class="form-error">{err.to_string()}</p> }.into_any(),
                    })
            }}
        </div>
    }
}

#[component]
fn TokenRow(token: ApiTokenInfo, delete_action: ServerAction<DeleteApiToken>) -> impl IntoView {
    let id = token.id.clone();
    let created = format_date(&token.created_at).to_string();
    let last_used = token
        .last_used_at
        .as_deref()
        .map(|value| format!(" · last used {}", format_date(value)));

    view! {
        <li class="token-row">
            <div>
                <p class="token-name">{token.name}</p>
                <p class="file-sub">"created " {created} {last_used}</p>
            </div>
            <button
                class="icon-btn-sm"
                title="revoke"
                on:click=move |_| {
                    delete_action.dispatch(DeleteApiToken { id: id.clone() });
                }
            >
                <TrashIcon/>
            </button>
        </li>
    }
}

#[component]
fn StorageSection() -> impl IntoView {
    let info = Resource::new(|| (), |_| get_storage_info());

    view! {
        <div class="settings-section">
            <h3>"storage"</h3>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    info.get()
                        .map(|result| match result {
                            Ok(info) => {
                                view! {
                                    <div class="storage-info">
                                        <p>
                                            <span class="settings-label">"backend"</span>
                                            {info.backend.clone()}
                                        </p>
                                        {info
                                            .bucket
                                            .clone()
                                            .map(|value| {
                                                view! {
                                                    <p>
                                                        <span class="settings-label">"bucket"</span>
                                                        {value}
                                                    </p>
                                                }
                                            })}
                                        {info
                                            .endpoint
                                            .clone()
                                            .map(|value| {
                                                view! {
                                                    <p>
                                                        <span class="settings-label">"endpoint"</span>
                                                        {value}
                                                    </p>
                                                }
                                            })}
                                        {info
                                            .root
                                            .clone()
                                            .map(|value| {
                                                view! {
                                                    <p>
                                                        <span class="settings-label">"path"</span>
                                                        {value}
                                                    </p>
                                                }
                                            })}
                                    </div>
                                }
                                    .into_any()
                            }
                            Err(_) => {
                                view! { <p class="form-error">"failed to load storage info"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
            <p class="settings-hint">
                "switching backends (e.g. to S3) is done with environment variables on the API server — see the README — not from here yet."
            </p>
        </div>
    }
}
