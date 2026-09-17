use leptos::prelude::*;

use crate::format::{format_date, format_size, format_type_breakdown};
use crate::icons::TrashIcon;
use crate::server_fns::{
    get_embed_settings, get_my_stats, get_storage_info, get_webhook_url, list_api_tokens, me,
    ApiTokenInfo, ChangePassword, ConfirmTotp, CreateApiToken, DeleteApiToken, DisableTotp,
    EmbedSettings, SetEmbedSettings, SetWebhookUrl, SetupTotp, TotpSetup,
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
            SettingsTab::Embeds => {
                view! {
                    <EmbedSection/>
                    <WebhookSection/>
                }
                    .into_any()
            }
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
fn WebhookSection() -> impl IntoView {
    let save_action = ServerAction::<SetWebhookUrl>::new();
    let current = Resource::new(move || save_action.version().get(), |_| get_webhook_url());

    view! {
        <div class="settings-section">
            <h3>"upload webhook"</h3>
            <p class="settings-hint">
                "post a notification to a Discord or Slack incoming webhook URL every time you "
                "upload a file. leave blank to turn this off."
            </p>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    current
                        .get()
                        .map(|result| match result {
                            Ok(webhook_url) => view! { <WebhookForm webhook_url save_action /> }
                                .into_any(),
                            Err(_) => {
                                view! { <p class="form-error">"failed to load webhook settings"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn WebhookForm(
    webhook_url: Option<String>,
    save_action: ServerAction<SetWebhookUrl>,
) -> impl IntoView {
    let (url, set_url) = signal(webhook_url.unwrap_or_default());

    view! {
        <form
            class="embed-form"
            on:submit=move |ev| {
                ev.prevent_default();
                let value = url.get();
                save_action
                    .dispatch(SetWebhookUrl {
                        webhook_url: (!value.trim().is_empty()).then_some(value),
                    });
            }
        >
            <div class="field">
                <label for="webhook-url">"webhook url"</label>
                <input
                    id="webhook-url"
                    type="text"
                    placeholder="https://discord.com/api/webhooks/..."
                    prop:value=move || url.get()
                    on:input=move |ev| set_url.set(event_target_value(&ev))
                />
            </div>
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
            <ChangePasswordForm/>
        </div>
        <TwoFactorSection/>
    }
}

#[component]
fn ChangePasswordForm() -> impl IntoView {
    let change_password = ServerAction::<ChangePassword>::new();
    let (current_password, set_current_password) = signal(String::new());
    let (new_password, set_new_password) = signal(String::new());
    let (confirm_password, set_confirm_password) = signal(String::new());
    let (mismatch, set_mismatch) = signal(false);

    view! {
        <form
            class="embed-form"
            on:submit=move |ev| {
                ev.prevent_default();
                if new_password.get() != confirm_password.get() {
                    set_mismatch.set(true);
                    return;
                }
                set_mismatch.set(false);
                change_password
                    .dispatch(ChangePassword {
                        current_password: current_password.get(),
                        new_password: new_password.get(),
                    });
                set_current_password.set(String::new());
                set_new_password.set(String::new());
                set_confirm_password.set(String::new());
            }
        >
            <div class="field">
                <label for="current-password">"current password"</label>
                <input
                    id="current-password"
                    type="password"
                    autocomplete="current-password"
                    prop:value=move || current_password.get()
                    on:input=move |ev| set_current_password.set(event_target_value(&ev))
                />
            </div>
            <div class="field">
                <label for="new-password">"new password"</label>
                <input
                    id="new-password"
                    type="password"
                    autocomplete="new-password"
                    prop:value=move || new_password.get()
                    on:input=move |ev| set_new_password.set(event_target_value(&ev))
                />
            </div>
            <div class="field">
                <label for="confirm-password">"confirm new password"</label>
                <input
                    id="confirm-password"
                    type="password"
                    autocomplete="new-password"
                    prop:value=move || confirm_password.get()
                    on:input=move |ev| set_confirm_password.set(event_target_value(&ev))
                />
            </div>
            <button type="submit" class="btn btn-primary">
                "change password"
            </button>
            {move || {
                mismatch
                    .get()
                    .then(|| view! { <p class="form-error">"new passwords don't match."</p> })
            }}
            {move || {
                change_password
                    .value()
                    .get()
                    .map(|result| match result {
                        Ok(_) => {
                            view! { <p class="settings-hint">"password changed."</p> }.into_any()
                        }
                        Err(err) => view! { <p class="form-error">{err.to_string()}</p> }.into_any(),
                    })
            }}
        </form>
    }
}

/// Its own top-level `settings-section` rather than folded into
/// `AccountSection` — it has enough moving state (setup in progress,
/// just-confirmed recovery codes, enabled-with-a-disable-form) that sharing
/// `AccountSection`'s own `Suspense`/resource would tangle the two.
#[component]
fn TwoFactorSection() -> impl IntoView {
    let setup_action = ServerAction::<SetupTotp>::new();
    let confirm_action = ServerAction::<ConfirmTotp>::new();
    let disable_action = ServerAction::<DisableTotp>::new();

    // Local to this section, not the shared `DashboardContext::account` —
    // that one's key never includes these actions (they don't touch
    // files/folders), and its own doc comment warns against folding
    // unrelated mutations into that resource's refresh key. A fresh,
    // independently-keyed resource is the same pattern every other
    // settings/admin section already uses (`RegistrationSection`,
    // `NamingSchemeSection`, etc.)
    let account = Resource::new(
        move || {
            (
                setup_action.version().get(),
                confirm_action.version().get(),
                disable_action.version().get(),
            )
        },
        |_| me(),
    );

    view! {
        <div class="settings-section">
            <h3>"two-factor authentication"</h3>
            {move || {
                confirm_action
                    .value()
                    .get()
                    .and_then(|result| result.ok())
                    .map(|codes| view! { <RecoveryCodes codes /> })
            }}
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    account
                        .get()
                        .map(|result| match result {
                            Ok(info) if info.totp_enabled => {
                                view! { <DisableTotpForm disable_action /> }.into_any()
                            }
                            Ok(_) => {
                                match setup_action.value().get() {
                                    Some(Ok(setup)) => {
                                        view! { <ConfirmTotpForm setup confirm_action /> }.into_any()
                                    }
                                    _ => view! { <EnableTotpPrompt setup_action /> }.into_any(),
                                }
                            }
                            Err(_) => {
                                view! { <p class="form-error">"failed to load 2FA status"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn EnableTotpPrompt(setup_action: ServerAction<SetupTotp>) -> impl IntoView {
    view! {
        <p class="settings-hint">
            "require a code from an authenticator app, in addition to your password, to sign in."
        </p>
        <button
            type="button"
            class="btn btn-primary"
            on:click=move |_| {
                setup_action.dispatch(SetupTotp {});
            }
        >
            "enable two-factor authentication"
        </button>
        {move || {
            setup_action
                .value()
                .get()
                .and_then(|result| result.err())
                .map(|err| view! { <p class="form-error">{err.to_string()}</p> })
        }}
    }
}

#[component]
fn ConfirmTotpForm(setup: TotpSetup, confirm_action: ServerAction<ConfirmTotp>) -> impl IntoView {
    let (code, set_code) = signal(String::new());
    let qr_src = format!("data:image/png;base64,{}", setup.qr_code_base64);
    let secret = setup.secret.clone();

    view! {
        <p class="settings-hint">
            "scan this with your authenticator app (Google Authenticator, 1Password, Authy, ...), "
            "or enter the code below manually:"
        </p>
        <img class="totp-qr" src=qr_src alt="two-factor setup QR code" />
        <p class="token-result">{secret}</p>
        <form
            class="embed-form"
            on:submit=move |ev| {
                ev.prevent_default();
                confirm_action.dispatch(ConfirmTotp { code: code.get() });
            }
        >
            <div class="field">
                <label for="confirm-totp-code">"code from your app"</label>
                <input
                    id="confirm-totp-code"
                    type="text"
                    inputmode="numeric"
                    autocomplete="one-time-code"
                    autofocus
                    on:input=move |ev| set_code.set(event_target_value(&ev))
                />
            </div>
            <button type="submit" class="btn btn-primary">
                "confirm and enable"
            </button>
            {move || {
                confirm_action
                    .value()
                    .get()
                    .and_then(|result| result.err())
                    .map(|err| view! { <p class="form-error">{err.to_string()}</p> })
            }}
        </form>
    }
}

#[component]
fn RecoveryCodes(codes: Vec<String>) -> impl IntoView {
    view! {
        <p class="settings-hint">
            "two-factor authentication is enabled. save these recovery codes somewhere safe — "
            "each works once, if you ever lose access to your authenticator app. they won't be "
            "shown again."
        </p>
        <div class="recovery-codes">
            {codes.into_iter().map(|code| view! { <span class="token-result">{code}</span> }).collect_view()}
        </div>
    }
}

#[component]
fn DisableTotpForm(disable_action: ServerAction<DisableTotp>) -> impl IntoView {
    let (disabling, set_disabling) = signal(false);
    let (password, set_password) = signal(String::new());
    let (code, set_code) = signal(String::new());

    view! {
        <p class="settings-row">
            "two-factor authentication is " <strong>"enabled"</strong> "."
        </p>
        {move || {
            if disabling.get() {
                view! {
                    <form
                        class="embed-form"
                        on:submit=move |ev| {
                            ev.prevent_default();
                            disable_action
                                .dispatch(DisableTotp {
                                    password: password.get(),
                                    code: code.get(),
                                });
                        }
                    >
                        <div class="field">
                            <label for="disable-totp-password">"current password"</label>
                            <input
                                id="disable-totp-password"
                                type="password"
                                autocomplete="current-password"
                                on:input=move |ev| set_password.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="field">
                            <label for="disable-totp-code">"code from your app, or a recovery code"</label>
                            <input
                                id="disable-totp-code"
                                type="text"
                                autocomplete="one-time-code"
                                on:input=move |ev| set_code.set(event_target_value(&ev))
                            />
                        </div>
                        <button type="submit" class="btn btn-danger">
                            "disable two-factor authentication"
                        </button>
                        <button
                            type="button"
                            class="btn btn-ghost"
                            on:click=move |_| set_disabling.set(false)
                        >
                            "cancel"
                        </button>
                        {move || {
                            disable_action
                                .value()
                                .get()
                                .and_then(|result| result.err())
                                .map(|err| view! { <p class="form-error">{err.to_string()}</p> })
                        }}
                    </form>
                }
                    .into_any()
            } else {
                view! {
                    <button
                        type="button"
                        class="btn btn-ghost"
                        on:click=move |_| set_disabling.set(true)
                    >
                        "disable two-factor authentication"
                    </button>
                }
                    .into_any()
            }
        }}
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
