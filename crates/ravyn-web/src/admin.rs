use leptos::prelude::*;

use crate::format::{format_date, format_size, format_type_breakdown, EXPIRY_PRESETS};
use crate::icons::TrashIcon;
use crate::server_fns::{
    get_admin_stats, get_instance_settings, list_invites, list_users, AdminUserInfo, CreateInvite,
    DeleteInvite, InstanceSettings, InviteInfo, SetInstanceSettings, SetUserLimit,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AdminTab {
    General,
    Users,
}

/// A separate page rather than a section of `/settings` — instance-wide
/// config an admin touches occasionally, as opposed to the personal
/// settings every user has. Gated on `is_admin` here too even though every
/// endpoint it calls already enforces that server-side: this just keeps a
/// non-admin from landing on a page of broken/forbidden requests if they
/// find the URL.
#[component]
pub fn AdminPage() -> impl IntoView {
    let account = expect_context::<crate::dashboard::DashboardContext>().account;

    view! {
        <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
            {move || {
                account
                    .get()
                    .map(|result| match result {
                        Ok(info) if info.is_admin => view! { <AdminTabs/> }.into_any(),
                        _ => {
                            view! {
                                <div class="section-head">
                                    <h2>"admin"</h2>
                                </div>
                                <p class="settings-hint">"you don't have access to this page."</p>
                            }
                                .into_any()
                        }
                    })
            }}
        </Suspense>
    }
}

#[component]
fn AdminTabs() -> impl IntoView {
    let tab = RwSignal::new(AdminTab::General);

    view! {
        <div class="section-head">
            <h2>"admin"</h2>
        </div>
        <div class="settings-tabs">
            <button
                class="settings-tab"
                class:active=move || tab.get() == AdminTab::General
                on:click=move |_| tab.set(AdminTab::General)
            >
                "general"
            </button>
            <button
                class="settings-tab"
                class:active=move || tab.get() == AdminTab::Users
                on:click=move |_| tab.set(AdminTab::Users)
            >
                "users"
            </button>
        </div>
        {move || match tab.get() {
            AdminTab::General => {
                view! {
                    <InstanceStatsSection/>
                    <RegistrationSection/>
                    <NamingSchemeSection/>
                    <ExpirySection/>
                }
                    .into_any()
            }
            AdminTab::Users => view! { <UsersSection/> }.into_any(),
        }}
    }
}

#[component]
fn InstanceStatsSection() -> impl IntoView {
    let stats = Resource::new(|| (), |_| get_admin_stats());

    view! {
        <div class="settings-section">
            <h3>"instance stats"</h3>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    stats
                        .get()
                        .map(|result| match result {
                            Ok(stats) => {
                                view! {
                                    <div class="stats-grid">
                                        <div class="stat-card">
                                            <span class="stat-value">{stats.total_users}</span>
                                            <p class="stat-label">"users"</p>
                                        </div>
                                        <div class="stat-card">
                                            <span class="stat-value">{stats.total_files}</span>
                                            <p class="stat-label">"files"</p>
                                        </div>
                                        <div class="stat-card">
                                            <span class="stat-value">
                                                {format_size(stats.total_storage_bytes.max(0) as u64)}
                                            </span>
                                            <p class="stat-label">"total storage"</p>
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
                                view! { <p class="form-error">"failed to load instance stats"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn RegistrationSection() -> impl IntoView {
    let mode_action = ServerAction::<SetInstanceSettings>::new();
    let settings = Resource::new(
        move || mode_action.version().get(),
        |_| get_instance_settings(),
    );

    view! {
        <div class="settings-section">
            <h3>"registration"</h3>
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
                        registration_mode: Some(mode.get()),
                        naming_scheme: None,
                        random_name_length: None,
                        default_expiry_preset: None,
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
fn NamingSchemeSection() -> impl IntoView {
    let scheme_action = ServerAction::<SetInstanceSettings>::new();
    let settings = Resource::new(
        move || scheme_action.version().get(),
        |_| get_instance_settings(),
    );

    view! {
        <div class="settings-section">
            <h3>"file naming"</h3>
            <p class="settings-hint">
                "what a freshly uploaded file gets called by default, instance-wide. "
                "anyone can still rename their own files afterward from the file's details."
            </p>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    settings
                        .get()
                        .map(|result| match result {
                            Ok(settings) => view! { <NamingSchemeForm settings scheme_action /> }
                                .into_any(),
                            Err(_) => {
                                view! { <p class="form-error">"failed to load naming settings"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn NamingSchemeForm(
    settings: InstanceSettings,
    scheme_action: ServerAction<SetInstanceSettings>,
) -> impl IntoView {
    let (scheme, set_scheme) = signal(settings.naming_scheme);
    let (length_input, set_length_input) = signal(settings.random_name_length.to_string());

    view! {
        <form
            class="embed-form"
            on:submit=move |ev| {
                ev.prevent_default();
                let length: i64 = length_input.get().trim().parse().unwrap_or(8);
                scheme_action
                    .dispatch(SetInstanceSettings {
                        registration_mode: None,
                        naming_scheme: Some(scheme.get()),
                        random_name_length: Some(length),
                        default_expiry_preset: None,
                    });
            }
        >
            <div class="field">
                <label for="naming-scheme">"naming scheme"</label>
                <select
                    id="naming-scheme"
                    class="folder-select"
                    on:change=move |ev| set_scheme.set(event_target_value(&ev))
                >
                    <option value="original" selected=move || scheme.get() == "original">
                        "original — keep the uploaded filename"
                    </option>
                    <option value="random" selected=move || scheme.get() == "random">
                        "random — a random string"
                    </option>
                    <option value="uuid" selected=move || scheme.get() == "uuid">
                        "uuid"
                    </option>
                    <option value="date" selected=move || scheme.get() == "date">
                        "date and time"
                    </option>
                </select>
            </div>
            {move || {
                (scheme.get() == "random")
                    .then(|| {
                        view! {
                            <div class="field">
                                <label for="random-name-length">"random name length"</label>
                                <input
                                    id="random-name-length"
                                    type="text"
                                    inputmode="numeric"
                                    prop:value=move || length_input.get()
                                    on:input=move |ev| set_length_input.set(event_target_value(&ev))
                                />
                            </div>
                        }
                    })
            }}
            <button type="submit" class="btn btn-primary">
                "save"
            </button>
            {move || {
                scheme_action
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
fn ExpirySection() -> impl IntoView {
    let expiry_action = ServerAction::<SetInstanceSettings>::new();
    let settings = Resource::new(
        move || expiry_action.version().get(),
        |_| get_instance_settings(),
    );

    view! {
        <div class="settings-section">
            <h3>"auto-delete"</h3>
            <p class="settings-hint">
                "how long a freshly uploaded file lives before it's deleted automatically, "
                "instance-wide. anyone can still pick a different expiry for their own file "
                "afterward from the file's details."
            </p>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    settings
                        .get()
                        .map(|result| match result {
                            Ok(settings) => view! { <ExpiryForm settings expiry_action /> }
                                .into_any(),
                            Err(_) => {
                                view! { <p class="form-error">"failed to load auto-delete settings"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn ExpiryForm(
    settings: InstanceSettings,
    expiry_action: ServerAction<SetInstanceSettings>,
) -> impl IntoView {
    let (preset, set_preset) = signal(settings.default_expiry_preset);

    view! {
        <form
            class="embed-form"
            on:submit=move |ev| {
                ev.prevent_default();
                expiry_action
                    .dispatch(SetInstanceSettings {
                        registration_mode: None,
                        naming_scheme: None,
                        random_name_length: None,
                        default_expiry_preset: Some(preset.get()),
                    });
            }
        >
            <div class="field">
                <label for="default-expiry-preset">"default expiry"</label>
                <select
                    id="default-expiry-preset"
                    class="folder-select"
                    on:change=move |ev| set_preset.set(event_target_value(&ev))
                >
                    {EXPIRY_PRESETS
                        .iter()
                        .map(|(value, label)| {
                            let value = value.to_string();
                            let value_for_select = value.clone();
                            let selected = move || preset.get() == value_for_select;
                            view! {
                                <option value=value selected=selected>
                                    {*label}
                                </option>
                            }
                        })
                        .collect_view()}
                </select>
            </div>
            <button type="submit" class="btn btn-primary">
                "save"
            </button>
            {move || {
                expiry_action
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

/// A storage quota per user (chibisafe/Zipline both call this a "limit" or
/// "quota") — the one admin control the instance actually needs day to day,
/// as opposed to a full user-management CRUD screen nobody self-hosting
/// this at their own scale is likely to need.
#[component]
fn UsersSection() -> impl IntoView {
    let limit_action = ServerAction::<SetUserLimit>::new();
    let users = Resource::new(move || limit_action.version().get(), |_| list_users());

    view! {
        <div class="settings-section">
            <h3>"users"</h3>
            <p class="settings-hint">
                "everyone with an account on this instance, and how much they've stored. "
                "leave a limit blank (or hit \"remove limit\") for unlimited."
            </p>
            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    users
                        .get()
                        .map(|result| match result {
                            Ok(users) => {
                                view! {
                                    <ul class="token-list">
                                        {users
                                            .into_iter()
                                            .map(|info| view! { <UserRow info limit_action /> })
                                            .collect_view()}
                                    </ul>
                                }
                                    .into_any()
                            }
                            Err(_) => {
                                view! { <p class="form-error">"failed to load users"</p> }.into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

const MIB: i64 = 1024 * 1024;

#[component]
fn UserRow(info: AdminUserInfo, limit_action: ServerAction<SetUserLimit>) -> impl IntoView {
    let id = info.id.clone();
    let id_for_clear = info.id.clone();
    let current_mib = info.max_storage_bytes.map(|bytes| (bytes / MIB).max(1));
    let (limit_input, set_limit_input) =
        signal(current_mib.map(|mib| mib.to_string()).unwrap_or_default());
    let used = format_size(info.storage_used_bytes.max(0) as u64);
    let limit_display = match info.max_storage_bytes {
        Some(bytes) => format_size(bytes.max(0) as u64),
        None => "unlimited".to_string(),
    };

    view! {
        <li class="token-row">
            <div>
                <p class="token-name">
                    {info.username.clone()}
                    {info.is_admin.then_some(" · admin")}
                </p>
                <p class="file-sub">
                    {used} " used of " {limit_display} " · " {info.file_count} " files"
                </p>
            </div>
            <div class="user-row-actions">
                <form
                    class="password-inline"
                    on:submit=move |ev| {
                        ev.prevent_default();
                        let mib: i64 = limit_input.get().trim().parse().unwrap_or(0);
                        limit_action
                            .dispatch(SetUserLimit {
                                id: id.clone(),
                                max_storage_bytes: (mib > 0).then_some(mib * MIB),
                            });
                    }
                >
                    <input
                        type="text"
                        inputmode="numeric"
                        placeholder="limit in MB"
                        prop:value=move || limit_input.get()
                        on:input=move |ev| set_limit_input.set(event_target_value(&ev))
                    />
                    <button type="submit" class="btn btn-ghost">
                        "save"
                    </button>
                </form>
                <button
                    class="btn btn-ghost"
                    on:click=move |_| {
                        limit_action
                            .dispatch(SetUserLimit {
                                id: id_for_clear.clone(),
                                max_storage_bytes: None,
                            });
                    }
                >
                    "remove limit"
                </button>
            </div>
        </li>
    }
}
