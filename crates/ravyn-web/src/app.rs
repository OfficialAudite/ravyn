use leptos::prelude::*;
use leptos_meta::{provide_meta_context, Link, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};

use crate::browser::{copy_to_clipboard, submit_input_form, sync_dropped_files};
use crate::format::{format_date, format_size};
use crate::icons::{CheckIcon, CopyIcon, FileTypeIcon, RavenIcon, TrashIcon};
use crate::server_fns::{list_files, CreateApiToken, DeleteFile, FileSummary, Login, Logout};

const FAVICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'%3E%3Cpath fill='%235b8dff' d='M2 17.5c2.8-6.2 6.7-9.3 10-9.3s7.2 3.1 10 9.3c-3.1-2.8-6.5-4.1-10-4.1s-6.9 1.3-10 4.1z'/%3E%3C/svg%3E";

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <meta name="theme-color" content="#0c0d12"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/ravyn-web.css"/>
        <Link rel="icon" href=FAVICON/>
        <Title text="ravyn"/>

        <Router>
            <Routes fallback=|| "page not found">
                <Route path=StaticSegment("") view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let login_action = ServerAction::<Login>::new();
    let logout_action = ServerAction::<Logout>::new();
    let delete_action = ServerAction::<DeleteFile>::new();

    // Re-fetching the file list is how the page reacts to auth state
    // changing: a login/logout/delete either succeeds (and the list
    // reloads) or fails (and the list read fails, which is what flips
    // between the login form and the dashboard below).
    let files = Resource::new(
        move || {
            (
                login_action.version().get(),
                logout_action.version().get(),
                delete_action.version().get(),
            )
        },
        |_| list_files(),
    );

    view! {
        <Suspense fallback=|| view! { <div class="login-screen"><p>"loading..."</p></div> }>
            {move || {
                files
                    .get()
                    .map(|result| match result {
                        Ok(files) => {
                            view! {
                                <main>
                                    <Dashboard files logout_action delete_action />
                                </main>
                            }
                                .into_any()
                        }
                        Err(_) => view! { <LoginForm login_action /> }.into_any(),
                    })
            }}
        </Suspense>
    }
}

#[component]
fn LoginForm(login_action: ServerAction<Login>) -> impl IntoView {
    let (username, set_username) = signal(String::new());
    let (password, set_password) = signal(String::new());

    view! {
        <div class="login-screen">
            <div class="login-card">
                <span class="wordmark">"ravyn"</span>
                <p class="login-tagline">"sign in to your hoard"</p>
                <form on:submit=move |ev| {
                    ev.prevent_default();
                    login_action
                        .dispatch(Login {
                            username: username.get(),
                            password: password.get(),
                        });
                }>
                    <div class="field">
                        <label for="username">"username"</label>
                        <input
                            id="username"
                            type="text"
                            autocomplete="username"
                            on:input=move |ev| set_username.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="field">
                        <label for="password">"password"</label>
                        <input
                            id="password"
                            type="password"
                            autocomplete="current-password"
                            on:input=move |ev| set_password.set(event_target_value(&ev))
                        />
                    </div>
                    <button type="submit" class="btn btn-primary btn-block">
                        "log in"
                    </button>
                    {move || {
                        login_action
                            .value()
                            .get()
                            .and_then(|result| result.err())
                            .map(|err| view! { <p class="form-error">{err.to_string()}</p> })
                    }}
                </form>
            </div>
        </div>
    }
}

#[component]
fn Dashboard(
    files: Vec<FileSummary>,
    logout_action: ServerAction<Logout>,
    delete_action: ServerAction<DeleteFile>,
) -> impl IntoView {
    let count = files.len();

    view! {
        <div class="topbar">
            <span class="wordmark">"ravyn"</span>
            <button
                class="btn btn-ghost"
                on:click=move |_| {
                    logout_action.dispatch(Logout {});
                }
            >
                "log out"
            </button>
        </div>

        <Dropzone/>

        <div class="section-head">
            <h2>"your hoard"</h2>
            <span class="count">{count}" file" {if count == 1 { "" } else { "s" }}</span>
        </div>

        {if files.is_empty() {
            view! {
                <div class="empty-state">
                    <RavenIcon class="raven"/>
                    <p>"nothing in the hoard yet — drop something above."</p>
                </div>
            }
                .into_any()
        } else {
            view! {
                <div class="file-grid">
                    {files
                        .into_iter()
                        .map(|file| view! { <FileCard file delete_action /> })
                        .collect_view()}
                </div>
            }
                .into_any()
        }}

        <div class="section-head">
            <h2>"api tokens"</h2>
        </div>
        <ApiTokenForm/>
    }
}

#[component]
fn Dropzone() -> impl IntoView {
    let (dragging, set_dragging) = signal(false);

    view! {
        <div
            class="dropzone"
            class:is-dragging=move || dragging.get()
            on:dragover=move |ev| {
                ev.prevent_default();
                set_dragging.set(true);
            }
            on:dragleave=move |_| set_dragging.set(false)
            on:drop=move |ev| {
                set_dragging.set(false);
                sync_dropped_files(ev, "file-input");
            }
        >
            <form method="post" action="/upload" enctype="multipart/form-data">
                <RavenIcon class="raven"/>
                <p class="dropzone-title">"drop files into the hoard"</p>
                <p class="dropzone-hint">"or click to choose"</p>
                <input
                    id="file-input"
                    type="file"
                    name="file"
                    required
                    on:change=move |_| submit_input_form("file-input")
                />
                <button type="submit" class="btn btn-ghost dropzone-submit">
                    "upload"
                </button>
            </form>
        </div>
    }
}

#[component]
fn FileCard(file: FileSummary, delete_action: ServerAction<DeleteFile>) -> impl IntoView {
    let (copied, set_copied) = signal(false);
    let is_image = file.content_type.starts_with("image/");

    let url = file.url.clone();
    let copy_url = file.url.clone();
    let id_for_delete = file.id.clone();
    let name = file.original_name.clone();
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
                    {if is_image {
                        view! { <img src=url.clone() alt=name.clone() loading="lazy" /> }.into_any()
                    } else {
                        view! { <FileTypeIcon content_type=content_type.clone() /> }.into_any()
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
                    <button
                        class="icon-btn"
                        on:click=move |_| {
                            delete_action.dispatch(DeleteFile { id: id_for_delete.clone() });
                        }
                        title="delete"
                    >
                        <TrashIcon/>
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

#[component]
fn ApiTokenForm() -> impl IntoView {
    let token_action = ServerAction::<CreateApiToken>::new();
    let (name, set_name) = signal(String::new());

    view! {
        <div class="token-panel">
            <p>"mint a token for ShareX or any other uploader — send it as a Bearer token."</p>
            <form
                class="token-form"
                on:submit=move |ev| {
                    ev.prevent_default();
                    token_action.dispatch(CreateApiToken { name: name.get() });
                }
            >
                <input
                    type="text"
                    placeholder="token name, e.g. ShareX"
                    on:input=move |ev| set_name.set(event_target_value(&ev))
                />
                <button type="submit" class="btn btn-primary">
                    "create token"
                </button>
            </form>
            {move || {
                token_action
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
