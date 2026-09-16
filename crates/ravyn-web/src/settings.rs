use leptos::prelude::*;

use crate::format::format_date;
use crate::icons::TrashIcon;
use crate::server_fns::{
    get_embed_settings, get_instance_settings, get_storage_info, list_api_tokens, list_invites, me,
    ApiTokenInfo, CreateApiToken, CreateInvite, DeleteApiToken, DeleteInvite, EmbedSettings,
    InstanceSettings, InviteInfo, SetEmbedSettings, SetInstanceSettings,
};

#[component]
pub fn SettingsPage() -> impl IntoView {
    view! {
        <div class="section-head">
            <h2>"settings"</h2>
        </div>
        <AccountSection/>
        <ApiTokensSection/>
        <EmbedSection/>
        <StorageSection/>
        <AdminSection/>
    }
}

/// Only an admin can see or touch registration settings — hidden entirely
/// for anyone else rather than shown-but-disabled, since a regular user has
/// no reason to know this exists.
#[component]
fn AdminSection() -> impl IntoView {
    let account = Resource::new(|| (), |_| me());

    view! {
        <Suspense fallback=|| ()>
            {move || {
                account
                    .get()
                    .map(|result| match result {
                        Ok(info) if info.is_admin => view! { <AdminControls/> }.into_any(),
                        _ => ().into_any(),
                    })
            }}
        </Suspense>
    }
}

#[component]
fn AdminControls() -> impl IntoView {
    let mode_action = ServerAction::<SetInstanceSettings>::new();
    let settings = Resource::new(
        move || mode_action.version().get(),
        |_| get_instance_settings(),
    );

    view! {
        <div class="settings-section">
            <h3>"admin"</h3>
            <p class="settings-hint">
                "control who can create an account on this instance. the very first account "
                "is always let through, regardless of this setting — otherwise there'd be no "
                "admin around to configure it."
            </p>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    settings
                        .get()
                        .map(|result| match result {
                            Ok(settings) => view! { <RegistrationModeForm settings mode_action /> }
                                .into_any(),
                            Err(_) => {
                                view! {
                                    <p class="form-error">"failed to load registration settings"</p>
                                }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn RegistrationModeForm(
    settings: InstanceSettings,
    mode_action: ServerAction<SetInstanceSettings>,
) -> impl IntoView {
    let (mode, set_mode) = signal(settings.registration_mode);

    view! {
        <form
            class="embed-form"
            on:submit=move |ev| {
                ev.prevent_default();
                mode_action
                    .dispatch(SetInstanceSettings {
                        registration_mode: mode.get(),
                    });
            }
        >
            <div class="field">
                <label for="registration-mode">"who can register"</label>
                <select
                    id="registration-mode"
                    class="folder-select"
                    on:change=move |ev| set_mode.set(event_target_value(&ev))
                >
                    <option value="closed" selected=move || mode.get() == "closed">
                        "closed — only via the CLI"
                    </option>
                    <option value="open" selected=move || mode.get() == "open">
                        "open — anyone can sign up"
                    </option>
                    <option value="invite" selected=move || mode.get() == "invite">
                        "invite-only"
                    </option>
                </select>
            </div>
            <button type="submit" class="btn btn-primary">
                "save"
            </button>
            {move || {
                mode_action
                    .value()
                    .get()
                    .map(|result| match result {
                        Ok(_) => view! { <p class="settings-hint">"saved."</p> }.into_any(),
                        Err(err) => view! { <p class="form-error">{err.to_string()}</p> }.into_any(),
                    })
            }}
        </form>
        {move || { (mode.get() == "invite").then(|| view! { <InvitesSection/> }) }}
    }
}

#[component]
fn InvitesSection() -> impl IntoView {
    let create_action = ServerAction::<CreateInvite>::new();
    let delete_action = ServerAction::<DeleteInvite>::new();

    let invites = Resource::new(
        move || (create_action.version().get(), delete_action.version().get()),
        |_| list_invites(),
    );

    view! {
        <div class="settings-section">
            <h3>"invite codes"</h3>
            <p class="settings-hint">"each code can be used once to create an account."</p>

            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    invites
                        .get()
                        .map(|result| match result {
                            Ok(invites) if invites.is_empty() => {
                                view! { <p class="settings-hint">"no invites yet."</p> }.into_any()
                            }
                            Ok(invites) => {
                                view! {
                                    <ul class="token-list">
                                        {invites
                                            .into_iter()
                                            .map(|invite| view! { <InviteRow invite delete_action /> })
                                            .collect_view()}
                                    </ul>
                                }
                                    .into_any()
                            }
                            Err(_) => {
                                view! { <p class="form-error">"failed to load invites"</p> }.into_any()
                            }
                        })
                }}
            </Suspense>

            <button
                class="btn btn-primary"
                on:click=move |_| {
                    create_action.dispatch(CreateInvite {});
                }
            >
                "generate invite code"
            </button>
            {move || {
                create_action
                    .value()
                    .get()
                    .map(|result| match result {
                        Ok(created) => {
                            view! {
                                <p class="token-result">
                                    "copy it now — it won't be shown again: " {created.token}
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
fn InviteRow(invite: InviteInfo, delete_action: ServerAction<DeleteInvite>) -> impl IntoView {
    let id = invite.id.clone();
    let created = format_date(&invite.created_at).to_string();

    view! {
        <li class="token-row">
            <div>
                <p class="token-name">{if invite.used { "used" } else { "unused" }}</p>
                <p class="file-sub">"created " {created}</p>
            </div>
            <button
                class="icon-btn-sm"
                title="revoke"
                on:click=move |_| {
                    delete_action.dispatch(DeleteInvite { id: id.clone() });
                }
            >
                <TrashIcon/>
            </button>
        </li>
    }
}

#[component]
fn EmbedSection() -> impl IntoView {
    let save_action = ServerAction::<SetEmbedSettings>::new();
    let settings = Resource::new(
        move || save_action.version().get(),
        |_| get_embed_settings(),
    );

    view! {
        <div class="settings-section">
            <h3>"discord embeds"</h3>
            <p class="settings-hint">
                "share links already preview images and videos directly, embeds off or on. "
                "turning this on adds a custom title, description, and accent color — what Discord, Slack, "
                "and Twitter show when someone pastes your link."
            </p>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    settings
                        .get()
                        .map(|result| match result {
                            Ok(settings) => view! { <EmbedForm settings save_action /> }.into_any(),
                            Err(_) => {
                                view! { <p class="form-error">"failed to load embed settings"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn EmbedForm(
    settings: EmbedSettings,
    save_action: ServerAction<SetEmbedSettings>,
) -> impl IntoView {
    let (enabled, set_enabled) = signal(settings.enabled);
    let (title, set_title) = signal(settings.title.unwrap_or_default());
    let (description, set_description) = signal(settings.description.unwrap_or_default());
    let (color, set_color) = signal(settings.color.unwrap_or_else(|| "#5b8dff".to_string()));
    let (site_name, set_site_name) = signal(settings.site_name.unwrap_or_default());

    view! {
        <form
            class="embed-form"
            on:submit=move |ev| {
                ev.prevent_default();
                save_action
                    .dispatch(SetEmbedSettings {
                        settings: EmbedSettings {
                            enabled: enabled.get(),
                            title: (!title.get().is_empty()).then(|| title.get()),
                            description: (!description.get().is_empty()).then(|| description.get()),
                            color: (!color.get().is_empty()).then(|| color.get()),
                            site_name: (!site_name.get().is_empty()).then(|| site_name.get()),
                        },
                    });
            }
        >
            <label class="embed-toggle">
                <input
                    type="checkbox"
                    prop:checked=move || enabled.get()
                    on:change=move |ev| set_enabled.set(event_target_checked(&ev))
                />
                "enable rich embeds"
            </label>

            <div class="field">
                <label for="embed-title">"title"</label>
                <input
                    id="embed-title"
                    type="text"
                    placeholder="{file.name}"
                    prop:value=move || title.get()
                    on:input=move |ev| set_title.set(event_target_value(&ev))
                />
            </div>
            <div class="field">
                <label for="embed-description">"description"</label>
                <input
                    id="embed-description"
                    type="text"
                    placeholder="uploaded by {user.username} · {file.size}"
                    prop:value=move || description.get()
                    on:input=move |ev| set_description.set(event_target_value(&ev))
                />
            </div>
            <div class="field">
                <label for="embed-site-name">"site name"</label>
                <input
                    id="embed-site-name"
                    type="text"
                    placeholder="ravyn"
                    prop:value=move || site_name.get()
                    on:input=move |ev| set_site_name.set(event_target_value(&ev))
                />
            </div>
            <div class="field">
                <label for="embed-color">"accent color"</label>
                <input
                    id="embed-color"
                    type="color"
                    prop:value=move || color.get()
                    on:input=move |ev| set_color.set(event_target_value(&ev))
                />
            </div>

            <p class="settings-hint">
                "available in title/description/site name: {file.name}, {file.size}, {file.type}, {user.username}"
            </p>

            <button type="submit" class="btn btn-primary">
                "save"
            </button>
            {move || {
                save_action
                    .value()
                    .get()
                    .map(|result| match result {
                        Ok(_) => view! { <p class="settings-hint">"saved."</p> }.into_any(),
                        Err(err) => view! { <p class="form-error">{err.to_string()}</p> }.into_any(),
                    })
            }}
        </form>
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
                                        {info.is_admin.then_some(" (admin)")}
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
                        Ok(created) => {
                            let download_href = format!(
                                "data:application/json;base64,{}",
                                base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    created.sharex_config.as_bytes(),
                                ),
                            );
                            view! {
                                <p class="token-result">
                                    "copy it now — it won't be shown again: " {created.token}
                                </p>
                                <a class="btn btn-ghost" href=download_href download="ravyn.sxcu">
                                    "download ShareX config"
                                </a>
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
