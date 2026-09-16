use leptos::prelude::*;
use leptos_router::components::{Outlet, A};

use crate::browser::{copy_to_clipboard, submit_input_form, sync_dropped_files};
use crate::format::{format_date, format_size};
use crate::icons::{
    CheckIcon, CloseIcon, CopyIcon, DownloadIcon, ExternalLinkIcon, FileTypeIcon, FolderIcon,
    LockIcon, PlusIcon, RavenIcon, SearchIcon, TrashIcon,
};
use crate::server_fns::{
    get_registration_status, list_files, list_folders, CreateFolder, DeleteFile, DeleteFolder,
    FileSummary, FolderSummary, Login, Logout, MoveFileToFolder, SetFilePassword,
    SetFolderPassword,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeFilter {
    All,
    Images,
    Videos,
    Audio,
    Documents,
    Other,
}

impl TypeFilter {
    fn matches(self, content_type: &str) -> bool {
        let is_image = content_type.starts_with("image/");
        let is_video = content_type.starts_with("video/");
        let is_audio = content_type.starts_with("audio/");
        let is_document = content_type == "application/pdf" || content_type.starts_with("text/");

        match self {
            TypeFilter::All => true,
            TypeFilter::Images => is_image,
            TypeFilter::Videos => is_video,
            TypeFilter::Audio => is_audio,
            TypeFilter::Documents => is_document,
            TypeFilter::Other => !(is_image || is_video || is_audio || is_document),
        }
    }

    fn label(self) -> &'static str {
        match self {
            TypeFilter::All => "all",
            TypeFilter::Images => "images",
            TypeFilter::Videos => "videos",
            TypeFilter::Audio => "audio",
            TypeFilter::Documents => "documents",
            TypeFilter::Other => "other",
        }
    }
}

const TYPE_FILTERS: [TypeFilter; 6] = [
    TypeFilter::All,
    TypeFilter::Images,
    TypeFilter::Videos,
    TypeFilter::Audio,
    TypeFilter::Documents,
    TypeFilter::Other,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortOrder {
    Newest,
    Oldest,
    NameAsc,
    SizeDesc,
}

impl SortOrder {
    fn apply(self, files: &mut [FileSummary]) {
        match self {
            SortOrder::Newest => files.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
            SortOrder::Oldest => files.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
            SortOrder::NameAsc => files.sort_by(|a, b| {
                a.original_name
                    .to_lowercase()
                    .cmp(&b.original_name.to_lowercase())
            }),
            SortOrder::SizeDesc => files.sort_by_key(|f| std::cmp::Reverse(f.size_bytes)),
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "oldest" => SortOrder::Oldest,
            "name" => SortOrder::NameAsc,
            "size" => SortOrder::SizeDesc,
            _ => SortOrder::Newest,
        }
    }
}

/// Every mutation that should trigger a refresh of the file/folder lists,
/// bundled so they can be wired into the resources' reactive dependencies
/// without an unwieldy tuple at every call site. Shared via context with
/// every page under [`DashboardLayout`].
#[derive(Clone, Copy)]
pub struct Actions {
    login: ServerAction<Login>,
    logout: ServerAction<Logout>,
    delete_file: ServerAction<DeleteFile>,
    move_file: ServerAction<MoveFileToFolder>,
    file_password: ServerAction<SetFilePassword>,
    create_folder: ServerAction<CreateFolder>,
    delete_folder: ServerAction<DeleteFolder>,
    folder_password: ServerAction<SetFolderPassword>,
}

#[derive(Clone, Copy)]
pub struct DashboardContext {
    pub actions: Actions,
    pub files: Resource<Result<Vec<FileSummary>, ServerFnError>>,
    pub folders: Resource<Result<Vec<FolderSummary>, ServerFnError>>,
    /// The id of the file whose detail modal is open, if any. Lives here —
    /// created once, in the top-level `DashboardLayout` component, which
    /// route navigation never tears down — rather than inside `BrowsePage`
    /// or `Browse`, both of which get rebuilt from scratch on every
    /// file/folder mutation (see the comment above `DashboardLayout`'s own
    /// `{move || ...}` block for why). A signal's *value* outlives whatever
    /// component tree happens to be reading it at the moment, as long as
    /// the signal itself was created somewhere stable — so the modal stays
    /// open across an edit made through its own controls without needing
    /// to change how or when the rest of the page re-renders.
    pub opened_file: RwSignal<Option<String>>,
}

/// The shared shell for the logged-in app: gates everything behind login,
/// then renders the top nav and lets the matched child route (browse,
/// upload, or settings) fill in the rest via `<Outlet/>`.
#[component]
pub fn DashboardLayout() -> impl IntoView {
    let actions = Actions {
        login: ServerAction::new(),
        logout: ServerAction::new(),
        delete_file: ServerAction::new(),
        move_file: ServerAction::new(),
        file_password: ServerAction::new(),
        create_folder: ServerAction::new(),
        delete_folder: ServerAction::new(),
        folder_password: ServerAction::new(),
    };

    let refresh_key = move || {
        (
            actions.login.version().get(),
            actions.logout.version().get(),
            actions.delete_file.version().get(),
            actions.move_file.version().get(),
            actions.file_password.version().get(),
            actions.create_folder.version().get(),
            actions.delete_folder.version().get(),
            actions.folder_password.version().get(),
        )
    };

    let files = Resource::new(refresh_key, |_| list_files());
    let folders = Resource::new(refresh_key, |_| list_folders());

    provide_context(DashboardContext {
        actions,
        files,
        folders,
        opened_file: RwSignal::new(None),
    });

    view! {
        <Suspense fallback=|| view! { <div class="login-screen"><p>"loading..."</p></div> }>
            {move || {
                files
                    .get()
                    .map(|result| match result {
                        Ok(_) => {
                            view! {
                                <main>
                                    <TopNav logout=actions.logout/>
                                    <Outlet/>
                                </main>
                            }
                                .into_any()
                        }
                        Err(_) => view! { <LoginForm login_action=actions.login /> }.into_any(),
                    })
            }}
        </Suspense>
    }
}

#[component]
fn TopNav(logout: ServerAction<Logout>) -> impl IntoView {
    view! {
        <div class="topbar">
            <span class="wordmark">"ravyn"</span>
            <nav class="top-nav">
                <A href="/" exact=true>
                    "browse"
                </A>
                <A href="/upload">"upload"</A>
                <A href="/settings">"settings"</A>
            </nav>
            <button
                class="btn btn-ghost"
                on:click=move |_| {
                    logout.dispatch(Logout {});
                }
            >
                "log out"
            </button>
        </div>
    }
}

#[component]
fn LoginForm(login_action: ServerAction<Login>) -> impl IntoView {
    let (username, set_username) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let status = Resource::new(|| (), |_| get_registration_status());

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
                // Its own `<Suspense>`, deliberately not sharing the outer
                // one gating this whole form on `files` — an extra resource
                // dropped into an already-Suspense-tracked subtree caused
                // exactly the hydration corruption described on
                // `DashboardContext::opened_file` above, just for the
                // login screen instead of the dashboard.
                <Suspense fallback=|| ()>
                    {move || {
                        status
                            .get()
                            .and_then(Result::ok)
                            .and_then(|status| {
                                if status.setup_required {
                                    Some(
                                        view! {
                                            <p class="login-tagline">
                                                <A href="/register">
                                                    "no account yet? set up ravyn"
                                                </A>
                                            </p>
                                        },
                                    )
                                } else if status.mode == "open" || status.mode == "invite" {
                                    Some(
                                        view! {
                                            <p class="login-tagline">
                                                <A href="/register">"create an account"</A>
                                            </p>
                                        },
                                    )
                                } else {
                                    None
                                }
                            })
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[component]
pub fn UploadPage() -> impl IntoView {
    view! {
        <div class="section-head">
            <h2>"upload"</h2>
        </div>
        <Dropzone/>
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
pub fn BrowsePage() -> impl IntoView {
    let ctx = expect_context::<DashboardContext>();

    view! {
        {move || {
            let files = ctx.files.get().and_then(Result::ok).unwrap_or_default();
            let folders = ctx.folders.get().and_then(Result::ok).unwrap_or_default();
            view! {
                <Browse files folders actions=ctx.actions opened_file=ctx.opened_file />
            }
        }}
    }
}

#[component]
fn Browse(
    files: Vec<FileSummary>,
    folders: Vec<FolderSummary>,
    actions: Actions,
    opened_file: RwSignal<Option<String>>,
) -> impl IntoView {
    let selected_folder = RwSignal::new(None::<String>);
    let search = RwSignal::new(String::new());
    let type_filter = RwSignal::new(TypeFilter::All);
    let sort_order = RwSignal::new(SortOrder::Newest);

    let visible_files = {
        let files = files.clone();
        move || {
            let mut visible: Vec<FileSummary> = files
                .iter()
                .filter(|f| match selected_folder.get() {
                    Some(folder_id) => f.folder_id.as_deref() == Some(folder_id.as_str()),
                    None => true,
                })
                .filter(|f| type_filter.get().matches(&f.content_type))
                .filter(|f| {
                    let query = search.get().to_lowercase();
                    query.is_empty() || f.original_name.to_lowercase().contains(&query)
                })
                .cloned()
                .collect();
            sort_order.get().apply(&mut visible);
            visible
        }
    };

    let folders_for_modal = folders.clone();
    let files_for_modal = files.clone();

    view! {
        <div class="workspace">
            <FolderSidebar folders=folders.clone() selected_folder actions/>

            <div class="workspace-main">
                <FilterBar search type_filter sort_order/>

                {move || {
                    let visible = visible_files();
                    if visible.is_empty() {
                        let message = if files.is_empty() {
                            view! {
                                <p>
                                    "nothing here yet — " <A href="/upload">"upload something"</A>
                                    " to get started."
                                </p>
                            }
                                .into_any()
                        } else {
                            view! { <p>"nothing matches — try adjusting your filters."</p> }
                                .into_any()
                        };
                        view! {
                            <div class="empty-state">
                                <RavenIcon class="raven"/>
                                {message}
                            </div>
                        }
                            .into_any()
                    } else {
                        view! {
                            <div class="file-grid">
                                {visible
                                    .into_iter()
                                    .map(|file| {
                                        view! { <FileCard file actions opened_file /> }
                                    })
                                    .collect_view()}
                            </div>
                        }
                            .into_any()
                    }
                }}
            </div>

            {move || {
                opened_file
                    .get()
                    .and_then(|id| files_for_modal.iter().find(|f| f.id == id).cloned())
                    .map(|file| {
                        view! {
                            <FileModal file folders=folders_for_modal.clone() actions opened_file />
                        }
                    })
            }}
        </div>
    }
}

#[component]
fn FolderSidebar(
    folders: Vec<FolderSummary>,
    selected_folder: RwSignal<Option<String>>,
    actions: Actions,
) -> impl IntoView {
    let (creating, set_creating) = signal(false);
    let (new_name, set_new_name) = signal(String::new());
    let (new_password, set_new_password) = signal(String::new());

    view! {
        <nav class="folder-sidebar">
            <button
                class="folder-tab"
                class:active=move || selected_folder.get().is_none()
                on:click=move |_| selected_folder.set(None)
            >
                <RavenIcon class="folder-tab-icon"/>
                "all files"
            </button>

            {folders
                .into_iter()
                .map(|folder| {
                    let id = folder.id.clone();
                    let id_for_click = id.clone();
                    let id_for_delete = id.clone();
                    view! {
                        <div class="folder-row">
                            <button
                                class="folder-tab"
                                class:active=move || selected_folder.get().as_deref() == Some(id.as_str())
                                on:click=move |_| selected_folder.set(Some(id_for_click.clone()))
                            >
                                <FolderIcon/>
                                {folder.name.clone()}
                                {folder.has_password.then(|| view! { <LockIcon/> })}
                            </button>
                            <button
                                class="icon-btn-sm"
                                title="delete folder"
                                on:click=move |_| {
                                    actions.delete_folder.dispatch(DeleteFolder { id: id_for_delete.clone() });
                                    selected_folder.set(None);
                                }
                            >
                                <TrashIcon/>
                            </button>
                        </div>
                    }
                })
                .collect_view()}

            {move || {
                if creating.get() {
                    view! {
                        <form
                            class="folder-create-form"
                            on:submit=move |ev| {
                                ev.prevent_default();
                                let password = new_password.get();
                                actions
                                    .create_folder
                                    .dispatch(CreateFolder {
                                        name: new_name.get(),
                                        password: (!password.is_empty()).then_some(password),
                                    });
                                set_creating.set(false);
                                set_new_name.set(String::new());
                                set_new_password.set(String::new());
                            }
                        >
                            <input
                                type="text"
                                placeholder="folder name"
                                autofocus
                                on:input=move |ev| set_new_name.set(event_target_value(&ev))
                            />
                            <input
                                type="password"
                                placeholder="password (optional)"
                                on:input=move |ev| set_new_password.set(event_target_value(&ev))
                            />
                            <button type="submit" class="btn btn-primary btn-block">
                                "create"
                            </button>
                        </form>
                    }
                        .into_any()
                } else {
                    view! {
                        <button class="folder-tab folder-tab-new" on:click=move |_| set_creating.set(true)>
                            <PlusIcon/>
                            "new folder"
                        </button>
                    }
                        .into_any()
                }
            }}
        </nav>
    }
}

#[component]
fn FilterBar(
    search: RwSignal<String>,
    type_filter: RwSignal<TypeFilter>,
    sort_order: RwSignal<SortOrder>,
) -> impl IntoView {
    view! {
        <div class="filter-bar">
            <div class="search-field">
                <SearchIcon/>
                <input
                    type="text"
                    placeholder="search files"
                    prop:value=move || search.get()
                    on:input=move |ev| search.set(event_target_value(&ev))
                />
            </div>

            <div class="type-tabs">
                {TYPE_FILTERS
                    .iter()
                    .map(|&filter| {
                        view! {
                            <button
                                class="type-tab"
                                class:active=move || type_filter.get() == filter
                                on:click=move |_| type_filter.set(filter)
                            >
                                {filter.label()}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>

            <select
                class="sort-select"
                on:change=move |ev| sort_order.set(SortOrder::from_str(&event_target_value(&ev)))
            >
                <option value="newest">"newest first"</option>
                <option value="oldest">"oldest first"</option>
                <option value="name">"name"</option>
                <option value="size">"largest first"</option>
            </select>
        </div>
    }
}

#[component]
fn FileCard(
    file: FileSummary,
    actions: Actions,
    opened_file: RwSignal<Option<String>>,
) -> impl IntoView {
    let (copied, set_copied) = signal(false);
    let (thumb_failed, set_thumb_failed) = signal(false);

    let is_image = file.content_type.starts_with("image/");
    let copy_url = file.url.clone();
    let thumbnail_url = file.thumbnail_url.clone();
    let id_for_delete = file.id.clone();
    let id_for_click = file.id.clone();
    let name = file.original_name.clone();
    let name_for_alt = name.clone();
    let content_type = file.content_type.clone();
    let size = file.size_bytes;
    let date = format_date(&file.created_at).to_string();
    let has_password = file.has_password;

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
                <button
                    type="button"
                    class="file-thumb-btn"
                    title="view details"
                    on:click=move |_| opened_file.set(Some(id_for_click.clone()))
                >
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
                </button>
                {has_password.then(|| view! { <div class="lock-badge"><LockIcon/></div> })}
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
                            actions.delete_file.dispatch(DeleteFile { id: id_for_delete.clone() });
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

/// The click-through detail view for a single file: a large preview plus
/// everything `FileCard` used to cram into the grid card itself —
/// categorizing into a folder, setting a password, copying the link,
/// deleting — the same "click a thumbnail, get a modal" shape chibisafe and
/// Zipline both use.
#[component]
fn FileModal(
    file: FileSummary,
    folders: Vec<FolderSummary>,
    actions: Actions,
    opened_file: RwSignal<Option<String>>,
) -> impl IntoView {
    let (copied, set_copied) = signal(false);
    let (password_input, set_password_input) = signal(String::new());

    let is_image = file.content_type.starts_with("image/");
    let is_video = file.content_type.starts_with("video/");
    let is_audio = file.content_type.starts_with("audio/");

    let copy_url = file.url.clone();
    let id_for_delete = file.id.clone();
    let id_for_move = file.id.clone();
    let id_for_password = file.id.clone();
    let current_folder = file.folder_id.clone();
    let has_password = file.has_password;
    let short_hash = file.sha256.get(..12).unwrap_or(&file.sha256).to_string();

    let close = move |_| opened_file.set(None);

    let copy = move |_| {
        copy_to_clipboard(&copy_url);
        set_copied.set(true);
        set_timeout(
            move || set_copied.set(false),
            std::time::Duration::from_millis(1500),
        );
    };

    view! {
        <div class="modal-backdrop" on:click=close>
            <div class="modal-panel" on:click=|ev| ev.stop_propagation()>
                <button type="button" class="modal-close" title="close" on:click=close>
                    <CloseIcon/>
                </button>

                <div class="modal-preview">
                    {if is_image {
                        view! {
                            <img src=file.raw_url.clone() alt=file.original_name.clone() />
                        }
                            .into_any()
                    } else if is_video {
                        view! { <video src=file.raw_url.clone() controls /> }.into_any()
                    } else if is_audio {
                        view! { <audio src=file.raw_url.clone() controls /> }.into_any()
                    } else {
                        view! { <FileTypeIcon content_type=file.content_type.clone() /> }.into_any()
                    }}
                </div>

                <div class="modal-body">
                    <h3 class="modal-title" title=file.original_name.clone()>
                        {file.original_name.clone()}
                    </h3>

                    <dl class="modal-meta">
                        <div>
                            <dt>"size"</dt>
                            <dd>{format_size(file.size_bytes)}</dd>
                        </div>
                        <div>
                            <dt>"type"</dt>
                            <dd>{file.content_type.clone()}</dd>
                        </div>
                        <div>
                            <dt>"uploaded"</dt>
                            <dd>{format_date(&file.created_at).to_string()}</dd>
                        </div>
                        <div>
                            <dt>"sha256"</dt>
                            <dd class="modal-hash" title=file.sha256.clone()>
                                {short_hash}"…"
                            </dd>
                        </div>
                    </dl>

                    <div class="field">
                        <label>"folder"</label>
                        <select
                            class="folder-select"
                            on:change=move |ev| {
                                let value = event_target_value(&ev);
                                let folder_id = (!value.is_empty()).then_some(value);
                                actions
                                    .move_file
                                    .dispatch(MoveFileToFolder {
                                        id: id_for_move.clone(),
                                        folder_id,
                                    });
                            }
                        >
                            <option value="" selected=current_folder.is_none()>
                                "no folder"
                            </option>
                            {folders
                                .into_iter()
                                .map(|folder| {
                                    let selected = current_folder.as_deref()
                                        == Some(folder.id.as_str());
                                    view! {
                                        <option value=folder.id.clone() selected=selected>
                                            {folder.name.clone()}
                                        </option>
                                    }
                                })
                                .collect_view()}
                        </select>
                    </div>

                    <form
                        class="password-inline"
                        on:submit=move |ev| {
                            ev.prevent_default();
                            let password = password_input.get();
                            actions
                                .file_password
                                .dispatch(SetFilePassword {
                                    id: id_for_password.clone(),
                                    password: (!password.is_empty()).then_some(password),
                                });
                            set_password_input.set(String::new());
                        }
                    >
                        <input
                            type="password"
                            placeholder=if has_password {
                                "change or clear password"
                            } else {
                                "set a password"
                            }
                            on:input=move |ev| set_password_input.set(event_target_value(&ev))
                        />
                        <button type="submit" class="btn btn-ghost">
                            "save"
                        </button>
                    </form>

                    <div class="modal-actions">
                        <button
                            class="icon-btn"
                            class:copied=move || copied.get()
                            on:click=copy
                            title="copy link"
                        >
                            {move || {
                                if copied.get() {
                                    view! { <CheckIcon/> }.into_any()
                                } else {
                                    view! { <CopyIcon/> }.into_any()
                                }
                            }}
                        </button>
                        <a
                            class="icon-btn"
                            href=file.raw_url.clone()
                            target="_blank"
                            title="open original"
                        >
                            <ExternalLinkIcon/>
                        </a>
                        <a
                            class="icon-btn"
                            href=file.raw_url.clone()
                            download=file.original_name.clone()
                            title="download"
                        >
                            <DownloadIcon/>
                        </a>
                        <button
                            class="icon-btn danger"
                            title="delete"
                            on:click=move |_| {
                                actions
                                    .delete_file
                                    .dispatch(DeleteFile { id: id_for_delete.clone() });
                                opened_file.set(None);
                            }
                        >
                            <TrashIcon/>
                        </button>
                    </div>
                </div>
            </div>
        </div>
    }
}
