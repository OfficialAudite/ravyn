use leptos::prelude::*;

use crate::format::{format_date, format_size, format_type_breakdown};
use crate::icons::TrashIcon;
use crate::server_fns::{
    get_embed_settings, get_my_stats, get_storage_info, list_api_tokens, ApiTokenInfo,
    CreateApiToken, DeleteApiToken, EmbedSettings, SetEmbedSettings,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    ApiTokens,
    Embeds,
    Storage,
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    let tab = RwSignal::new(SettingsTab::General);

    view! {
        <div class="section-head">
            <h2>"settings"</h2>
        </div>
        <div class="settings-tabs">
            <button
                class="settings-tab"
                class:active=move || tab.get() == SettingsTab::General
                on:click=move |_| tab.set(SettingsTab::General)
            >
                "general"
            </button>
            <button
                class="settings-tab"
                class:active=move || tab.get() == SettingsTab::ApiTokens
                on:click=move |_| tab.set(SettingsTab::ApiTokens)
            >
                "api tokens"
            </button>
            <button
                class="settings-tab"
                class:active=move || tab.get() == SettingsTab::Embeds
                on:click=move |_| tab.set(SettingsTab::Embeds)
            >
                "embeds"
            </button>
            <button
                class="settings-tab"
                class:active=move || tab.get() == SettingsTab::Storage
                on:click=move |_| tab.set(SettingsTab::Storage)
            >
                "storage"
            </button>
        </div>
        {move || match tab.get() {
            SettingsTab::General => {
                view! {
                    <AccountSection/>
                    <MyStatsSection/>
                }
                    .into_any()
            }
            SettingsTab::ApiTokens => view! { <ApiTokensSection/> }.into_any(),
            SettingsTab::Embeds => view! { <EmbedSection/> }.into_any(),
            SettingsTab::Storage => view! { <StorageSection/> }.into_any(),
        }}
    }
}

#[component]
fn MyStatsSection() -> impl IntoView {
    let stats = Resource::new(|| (), |_| get_my_stats());

    view! {
        <div class="settings-section">
            <h3>"your stats"</h3>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    stats
                        .get()
                        .map(|result| match result {
                            Ok(stats) => {
                                let limit_label = match stats.max_storage_bytes {
                                    Some(bytes) => {
                                        format!("of {}", format_size(bytes.max(0) as u64))
                                    }
                                    None => "unlimited".to_string(),
                                };
                                view! {
                                    <div class="stats-grid">
                                        <div class="stat-card">
                                            <span class="stat-value">
                                                {format_size(stats.storage_used_bytes.max(0) as u64)}
                                            </span>
                                            <p class="stat-label">{limit_label}</p>
                                        </div>
                                        <div class="stat-card">
                                            <span class="stat-value">{stats.file_count}</span>
                                            <p class="stat-label">"files"</p>
                                        </div>
                                    </div>
                                    <p class="stats-breakdown">
                                        {format_type_breakdown(
                                            stats.by_type.images,
                                            stats.by_type.videos,
                                            stats.by_type.audio,
                                            stats.by_type.documents,
                                            stats.by_type.other,
                                        )}
                                    </p>
                                }
                                    .into_any()
                            }
                            Err(_) => {
                                view! { <p class="form-error">"failed to load your stats"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
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
    let account = expect_context::<crate::dashboard::DashboardContext>().account;

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
