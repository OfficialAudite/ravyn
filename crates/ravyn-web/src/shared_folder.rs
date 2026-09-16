use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::browser::copy_to_clipboard;
use crate::format::{format_date, format_size};
use crate::icons::{CheckIcon, CopyIcon, FileTypeIcon, RavenIcon};
use crate::server_fns::{get_shared_folder, FileSummary};

#[component]
pub fn SharedFolderPage() -> impl IntoView {
    let params = use_params_map();
    let id = move || params.get().get("id").unwrap_or_default();
    let password = RwSignal::new(None::<String>);

    let folder = Resource::new(
        move || (id(), password.get()),
        |(id, password)| async move { get_shared_folder(id, password).await },
    );

    view! {
        <main>
            <Suspense fallback=|| view! { <div class="login-screen"><p>"loading..."</p></div> }>
                {move || {
                    folder
                        .get()
                        .map(|result| match result {
                            Ok(shared) => view! { <SharedFolderView shared /> }.into_any(),
                            Err(_) => view! { <PasswordGate password /> }.into_any(),
                        })
                }}
            </Suspense>
        </main>
    }
}

#[component]
fn PasswordGate(password: RwSignal<Option<String>>) -> impl IntoView {
    let (input, set_input) = signal(String::new());

    view! {
        <div class="login-screen">
            <div class="login-card">
                <span class="wordmark">"ravyn"</span>
                <p class="login-tagline">"this folder is password protected"</p>
                <form on:submit=move |ev| {
                    ev.prevent_default();
                    password.set(Some(input.get()));
                }>
                    <div class="field">
                        <label for="folder-password">"password"</label>
                        <input
                            id="folder-password"
                            type="password"
                            autofocus
                            on:input=move |ev| set_input.set(event_target_value(&ev))
                        />
                    </div>
                    <button type="submit" class="btn btn-primary btn-block">
                        "unlock"
                    </button>
                </form>
            </div>
        </div>
    }
}

#[component]
fn SharedFolderView(shared: crate::server_fns::SharedFolder) -> impl IntoView {
    let count = shared.files.len();

    view! {
        <div class="topbar">
            <span class="wordmark">"ravyn"</span>
            <span class="folder-view-title">{shared.name}</span>
        </div>

        <div class="section-head">
            <h2>"shared folder"</h2>
            <span class="count">{count}" file" {if count == 1 { "" } else { "s" }}</span>
        </div>

        {if shared.files.is_empty() {
            view! {
                <div class="empty-state">
                    <RavenIcon class="raven"/>
                    <p>"this folder is empty."</p>
                </div>
            }
                .into_any()
        } else {
            view! {
                <div class="file-grid">
                    {shared.files.into_iter().map(|file| view! { <SharedFileCard file /> }).collect_view()}
                </div>
            }
                .into_any()
        }}
    }
}

#[component]
fn SharedFileCard(file: FileSummary) -> impl IntoView {
    let (copied, set_copied) = signal(false);
    let (thumb_failed, set_thumb_failed) = signal(false);

    let is_image = file.content_type.starts_with("image/");
    let url = file.url.clone();
    let copy_url = file.url.clone();
    let thumbnail_url = file.thumbnail_url.clone();
    let name = file.original_name.clone();
    let name_for_alt = name.clone();
    let content_type = file.content_type.clone();
    let size = file.size_bytes;
    let date = format_date(&file.created_at).to_string();

    let copy = move |_| {
        copy_to_clipboard(&copy_url);
        set_copied.set(true);
        set_timeout(
            move || set_copied.set(false),
            std::time::Duration::from_millis(1500),
        );
    };

    view! {
        <div class="file-card">
            <div class="file-thumb">
                <a href=url.clone() target="_blank" title="open">
                    {move || {
                        if is_image && !thumb_failed.get() {
                            view! {
                                <img
                                    src=thumbnail_url.clone()
                                    alt=name_for_alt.clone()
                                    loading="lazy"
                                    on:error=move |_| set_thumb_failed.set(true)
                                />
                            }
                                .into_any()
                        } else {
                            view! { <FileTypeIcon content_type=content_type.clone() /> }.into_any()
                        }
                    }}
                </a>
                <div class="file-actions">
                    <button
                        class="icon-btn"
                        class:copied=move || copied.get()
                        on:click=copy
                        title="copy link"
                    >
                        {move || {
                            if copied.get() {
                                view! { <CheckIcon /> }.into_any()
                            } else {
                                view! { <CopyIcon /> }.into_any()
                            }
                        }}
                    </button>
                </div>
            </div>
            <div class="file-meta">
                <p class="file-name" title=name.clone()>
                    {name.clone()}
                </p>
                <p class="file-sub">{format_size(size)}" · "{date}</p>
            </div>
        </div>
    }
}
