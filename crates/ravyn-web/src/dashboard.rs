use leptos::prelude::*;
use leptos_router::components::{Outlet, A};

use crate::browser::{
    copy_to_clipboard, paste_text_and_submit, submit_input_form, sync_dropped_files,
};
use crate::format::{format_date, format_size, EXPIRY_PRESETS};
use crate::icons::{
    CheckIcon, ClockIcon, CloseIcon, CopyIcon, DownloadIcon, ExternalLinkIcon, FileTypeIcon,
    FolderIcon, LockIcon, PencilIcon, PlusIcon, RavenIcon, SearchIcon, TagIcon, TrashIcon,
};
use crate::server_fns::{
    get_registration_status, list_files, list_folders, list_short_urls, me, AccountInfo,
    CreateFolder, CreateShortUrl, DeleteFile, DeleteFolder, DeleteShortUrl, FileSummary,
    FolderSummary, Login, LoginResult, LoginTotp, Logout, MoveFileToFolder, RenameFile,
    SetFileExpiry, SetFilePassword, SetFileTags, SetFolderPassword, ShortUrlInfo,
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
    login_totp: ServerAction<LoginTotp>,
    logout: ServerAction<Logout>,
    delete_file: ServerAction<DeleteFile>,
    move_file: ServerAction<MoveFileToFolder>,
    file_password: ServerAction<SetFilePassword>,
    rename_file: ServerAction<RenameFile>,
    file_expiry: ServerAction<SetFileExpiry>,
    file_tags: ServerAction<SetFileTags>,
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
    /// Fetched once here, alongside `files`/`folders` — not by `TopNav`
    /// itself, even though it's the only thing that reads it, so its fetch
    /// doesn't restart every time `<main>` rebuilds (see `opened_file`
    /// above for why that happens on every file/folder mutation). This
    /// alone doesn't make `TopNav`'s use of it perfectly clean: `<main>`,
    /// and the `<Suspense>` around this resource's read inside `TopNav`,
    /// still get torn down and recreated on the same mutations, and that
    /// occasionally logs a harmless wasm "closure invoked ... after being
    /// dropped" to the console (confirmed by testing: it never affects
    /// what actually renders). Removing that `<Suspense>` looks tempting —
    /// don't: without it, the client's first frame can transiently read
    /// this resource as unresolved before the server's already-resolved
    /// render catches up, which mismatches hydration for real and produces
    /// an actual crash, not just a log line. Properly fixing the underlying
    /// `<main>`-rebuilds-on-every-mutation issue is the real fix, but two
    /// earlier attempts at that (a `Memo` gate, a second gating resource)
    /// each made hydration fail far worse — left alone here as the smaller
    /// of the known evils.
    pub account: Resource<Result<AccountInfo, ServerFnError>>,
}

/// The shared shell for the logged-in app: gates everything behind login,
/// then renders the top nav and lets the matched child route (browse,
/// upload, or settings) fill in the rest via `<Outlet/>`.
#[component]
pub fn DashboardLayout() -> impl IntoView {
    let actions = Actions {
        login: ServerAction::new(),
        login_totp: ServerAction::new(),
        logout: ServerAction::new(),
        delete_file: ServerAction::new(),
        move_file: ServerAction::new(),
        file_password: ServerAction::new(),
        rename_file: ServerAction::new(),
        file_expiry: ServerAction::new(),
        file_tags: ServerAction::new(),
        create_folder: ServerAction::new(),
        delete_folder: ServerAction::new(),
        folder_password: ServerAction::new(),
    };

    let refresh_key = move || {
        (
            actions.login.version().get(),
            actions.login_totp.version().get(),
            actions.logout.version().get(),
            actions.delete_file.version().get(),
            actions.move_file.version().get(),
            actions.file_password.version().get(),
            actions.rename_file.version().get(),
            actions.file_expiry.version().get(),
            actions.file_tags.version().get(),
            actions.create_folder.version().get(),
            actions.delete_folder.version().get(),
            actions.folder_password.version().get(),
        )
    };

    let files = Resource::new(refresh_key, |_| list_files());
    let folders = Resource::new(refresh_key, |_| list_folders());
    // Keyed on `refresh_key`, not `|| ()`: fetched once at mount it would
    // run before login (no session cookie yet), cache that `Err`, and never
    // refetch — the admin nav link would then never appear until a full
    // page reload created a fresh `DashboardLayout` (and thus a fresh
    // resource) with the cookie already in place. Refetching alongside
    // `files`/`folders` on every login keeps it in sync with who's
    // actually signed in.
    let account = Resource::new(refresh_key, |_| me());

    provide_context(DashboardContext {
        actions,
        files,
        folders,
        opened_file: RwSignal::new(None),
        account,
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
                        Err(_) => {
                            view! {
                                <LoginForm
                                    login_action=actions.login
                                    login_totp_action=actions.login_totp
                                />
                            }
                                .into_any()
                        }
                    })
            }}
        </Suspense>
    }
}

#[component]
fn TopNav(logout: ServerAction<Logout>) -> impl IntoView {
    let ctx = expect_context::<DashboardContext>();
    let account = ctx.account;

    view! {
        <div class="topbar">
            <span class="wordmark">"ravyn"</span>
            <nav class="top-nav">
                <A href="/" exact=true>
                    "browse"
                </A>
                <A href="/upload">"upload"</A>
                <A href="/settings">"settings"</A>
                <Suspense fallback=|| ()>
                    {move || {
                        account
                            .get()
                            .map(|result| match result {
                                Ok(info) if info.is_admin => {
                                    view! { <A href="/admin">"admin"</A> }.into_any()
                                }
                                _ => ().into_any(),
                            })
                    }}
                </Suspense>
            </nav>
            <button
                class="btn btn-ghost"
                on:click=move |_| {
                    // `login`'s resolved value otherwise lingers as
                    // `TotpRequired` from whatever login got this session
                    // started, so logging back in later would show the
                    // code-entry screen before a password was ever typed.
                    ctx.actions.login.clear();
                    logout.dispatch(Logout {});
                }
            >
                "log out"
            </button>
        </div>
    }
}

#[component]
fn LoginForm(
    login_action: ServerAction<Login>,
    login_totp_action: ServerAction<LoginTotp>,
) -> impl IntoView {
    let (username, set_username) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (totp_code, set_totp_code) = signal(String::new());
    let status = Resource::new(|| (), |_| get_registration_status());

    // A correct password on a 2FA account doesn't sign you in — `login`
    // returns `TotpRequired { login_token }` instead of setting a cookie,
    // and this reads that straight back out of the action's own resolved
    // value rather than a separate signal, so there's nothing to keep in
    // sync if the action ever resolves more than once.
    let pending_login_token = move || match login_action.value().get() {
        Some(Ok(LoginResult::TotpRequired { login_token })) => Some(login_token),
        _ => None,
    };

    view! {
        <div class="login-screen">
            <div class="login-card">
                <span class="wordmark">"ravyn"</span>
                {move || {
                    if let Some(login_token) = pending_login_token() {
                        view! {
                            <p class="login-tagline">
                                "enter the code from your authenticator app"
                            </p>
                            <form on:submit=move |ev| {
                                ev.prevent_default();
                                login_totp_action
                                    .dispatch(LoginTotp {
                                        login_token: login_token.clone(),
                                        code: totp_code.get(),
                                    });
                            }>
                                <div class="field">
                                    <label for="totp-code">"code"</label>
                                    <input
                                        id="totp-code"
                                        type="text"
                                        inputmode="numeric"
                                        autocomplete="one-time-code"
                                        autofocus
                                        on:input=move |ev| set_totp_code.set(event_target_value(&ev))
                                    />
                                </div>
                                <button type="submit" class="btn btn-primary btn-block">
                                    "verify"
                                </button>
                                {move || {
                                    login_totp_action
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
                        }
                            .into_any()
                    }
                }}
            </div>
        </div>
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UploadMode {
    Files,
    Paste,
    Shorten,
}

#[component]
pub fn UploadPage() -> impl IntoView {
    let mode = RwSignal::new(UploadMode::Files);

    view! {
        <div class="section-head">
            <h2>"upload"</h2>
        </div>
        <div class="settings-tabs">
            <button
                class="settings-tab"
                class:active=move || mode.get() == UploadMode::Files
                on:click=move |_| mode.set(UploadMode::Files)
            >
                "files"
            </button>
            <button
                class="settings-tab"
                class:active=move || mode.get() == UploadMode::Paste
                on:click=move |_| mode.set(UploadMode::Paste)
            >
                "paste text"
            </button>
            <button
                class="settings-tab"
                class:active=move || mode.get() == UploadMode::Shorten
                on:click=move |_| mode.set(UploadMode::Shorten)
            >
                "shorten url"
            </button>
        </div>
        {move || match mode.get() {
            UploadMode::Files => view! { <Dropzone/> }.into_any(),
            UploadMode::Paste => view! { <PasteText/> }.into_any(),
            UploadMode::Shorten => view! { <ShortenUrl/> }.into_any(),
        }}
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
                <p class="dropzone-hint">"or click to choose — multiple at once is fine"</p>
                <input
                    id="file-input"
                    type="file"
                    name="file"
                    multiple
                    required
                    on:change=move |_| submit_input_form("file-input")
                />
                <noscript>
                    <button type="submit" class="btn btn-ghost dropzone-submit">
                        "upload"
                    </button>
                </noscript>
            </form>
        </div>
    }
}

/// A pastebin-style alternative to picking a file: builds a synthetic text
/// file client-side (`paste_text_and_submit`) and pushes it through the
/// exact same `/upload` multipart pipeline `Dropzone` uses, so it needs no
/// server-side changes of its own — naming, quotas, and expiry all just
/// work. JS-only (unlike `Dropzone`, no `<noscript>` fallback makes sense
/// here: there's no plain-HTML way to turn typed text into an uploadable
/// file).
#[component]
fn PasteText() -> impl IntoView {
    let (text, set_text) = signal(String::new());
    let (filename, set_filename) = signal(String::new());

    let submit = move |_| {
        let content = text.get();
        if content.trim().is_empty() {
            return;
        }
        let name = filename.get();
        let name = match name.trim() {
            "" => "paste.txt".to_string(),
            name if name.contains('.') => name.to_string(),
            name => format!("{name}.txt"),
        };
        paste_text_and_submit("paste-file-input", &name, &content);
    };

    view! {
        <div class="paste-text">
            <form method="post" action="/upload" enctype="multipart/form-data">
                <input
                    type="text"
                    class="paste-filename"
                    placeholder="filename (optional, e.g. notes.txt)"
                    on:input=move |ev| set_filename.set(event_target_value(&ev))
                />
                <textarea
                    class="paste-textarea"
                    placeholder="paste or type your text here..."
                    on:input=move |ev| set_text.set(event_target_value(&ev))
                >
                </textarea>
                <input id="paste-file-input" class="hidden-file-input" type="file" name="file" />
                <button type="button" class="btn btn-primary" on:click=submit>
                    "create paste"
                </button>
            </form>
        </div>
    }
}

/// Same "create, then a list of your own below it" shape as
/// `FolderSidebar`'s create-a-folder form, just without needing a modal or
/// a separate page — a shortened link has nothing else to configure.
#[component]
fn ShortenUrl() -> impl IntoView {
    let create_action = ServerAction::<CreateShortUrl>::new();
    let delete_action = ServerAction::<DeleteShortUrl>::new();
    let (destination, set_destination) = signal(String::new());

    let short_urls = Resource::new(
        move || (create_action.version().get(), delete_action.version().get()),
        |_| list_short_urls(),
    );

    view! {
        <div class="paste-text">
            <form
                class="embed-form"
                on:submit=move |ev| {
                    ev.prevent_default();
                    let url = destination.get();
                    if !url.trim().is_empty() {
                        create_action.dispatch(CreateShortUrl { destination: url });
                        set_destination.set(String::new());
                    }
                }
            >
                <div class="field">
                    <label for="shorten-destination">"url to shorten"</label>
                    <input
                        id="shorten-destination"
                        type="text"
                        placeholder="https://example.com/a/very/long/path"
                        prop:value=move || destination.get()
                        on:input=move |ev| set_destination.set(event_target_value(&ev))
                    />
                </div>
                <button type="submit" class="btn btn-primary">
                    "shorten"
                </button>
                {move || {
                    create_action
                        .value()
                        .get()
                        .map(|result| match result {
                            Ok(created) => {
                                view! { <p class="token-result">{created.short_url}</p> }.into_any()
                            }
                            Err(err) => view! { <p class="form-error">{err.to_string()}</p> }.into_any(),
                        })
                }}
            </form>

            <Suspense fallback=|| view! { <p class="settings-hint">"loading..."</p> }>
                {move || {
                    short_urls
                        .get()
                        .map(|result| match result {
                            Ok(urls) if urls.is_empty() => {
                                view! { <p class="settings-hint">"no shortened links yet."</p> }
                                    .into_any()
                            }
                            Ok(urls) => {
                                view! {
                                    <ul class="token-list">
                                        {urls
                                            .into_iter()
                                            .map(|info| view! { <ShortUrlRow info delete_action /> })
                                            .collect_view()}
                                    </ul>
                                }
                                    .into_any()
                            }
                            Err(_) => {
                                view! { <p class="form-error">"failed to load short links"</p> }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn ShortUrlRow(info: ShortUrlInfo, delete_action: ServerAction<DeleteShortUrl>) -> impl IntoView {
    let id = info.id.clone();
    let copy_url = info.short_url.clone();
    let (copied, set_copied) = signal(false);

    let copy = move |_| {
        copy_to_clipboard(&copy_url);
        set_copied.set(true);
        set_timeout(
            move || set_copied.set(false),
            std::time::Duration::from_millis(1500),
        );
    };

    view! {
        <li class="token-row">
            <div>
                <p class="token-name">{info.short_url.clone()}</p>
                <p class="file-sub">
                    {info.destination.clone()} " · " {info.clicks} " clicks"
                </p>
            </div>
            <div class="user-row-actions">
                <button
                    class="icon-btn-sm"
                    class:copied=move || copied.get()
                    title="copy link"
                    on:click=copy
                >
                    {move || {
                        if copied.get() {
                            view! { <CheckIcon/> }.into_any()
                        } else {
                            view! { <CopyIcon/> }.into_any()
                        }
                    }}
                </button>
                <button
                    class="icon-btn-sm"
                    title="delete"
                    on:click=move |_| {
                        delete_action.dispatch(DeleteShortUrl { id: id.clone() });
                    }
                >
                    <TrashIcon/>
                </button>
            </div>
        </li>
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
    let select_mode = RwSignal::new(false);
    let selected = RwSignal::new(std::collections::HashSet::<String>::new());

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
                    query.is_empty()
                        || f.original_name.to_lowercase().contains(&query)
                        || f.tags.iter().any(|tag| tag.to_lowercase().contains(&query))
                })
                .cloned()
                .collect();
            sort_order.get().apply(&mut visible);
            visible
        }
    };

    let folders_for_modal = folders.clone();
    let folders_for_bulk = folders.clone();
    let files_for_modal = files.clone();

    view! {
        <div class="workspace">
            <FolderSidebar folders=folders.clone() selected_folder actions/>

            <div class="workspace-main">
                <FilterBar search type_filter sort_order select_mode selected/>

                {move || {
                    select_mode
                        .get()
                        .then(|| {
                            view! {
                                <BulkActionsBar
                                    selected
                                    select_mode
                                    folders=folders_for_bulk.clone()
                                    actions
                                />
                            }
                        })
                }}

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
                                        view! {
                                            <FileCard
                                                file
                                                actions
                                                opened_file
                                                search
                                                select_mode
                                                selected
                                            />
                                        }
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
                            <div class="folder-create-actions">
                                <button type="submit" class="btn btn-primary">
                                    "create"
                                </button>
                                <button
                                    type="button"
                                    class="btn btn-ghost"
                                    on:click=move |_| {
                                        set_creating.set(false);
                                        set_new_name.set(String::new());
                                        set_new_password.set(String::new());
                                    }
                                >
                                    "cancel"
                                </button>
                            </div>
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
    select_mode: RwSignal<bool>,
    selected: RwSignal<std::collections::HashSet<String>>,
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

            <button
                type="button"
                class="btn btn-ghost"
                class:active=move || select_mode.get()
                on:click=move |_| {
                    if select_mode.get() {
                        selected.set(Default::default());
                    }
                    select_mode.update(|mode| *mode = !*mode);
                }
            >
                {move || if select_mode.get() { "cancel select" } else { "select" }}
            </button>
        </div>
    }
}

/// Only mounted while `select_mode` is on (`Browse`'s own `{move || ...}`
/// gate) — everything here acts on `selected`, so there's nothing useful
/// to show while it's necessarily empty.
#[component]
fn BulkActionsBar(
    selected: RwSignal<std::collections::HashSet<String>>,
    select_mode: RwSignal<bool>,
    folders: Vec<FolderSummary>,
    actions: Actions,
) -> impl IntoView {
    let clear = move || {
        selected.set(Default::default());
        select_mode.set(false);
    };

    view! {
        <div class="bulk-actions-bar">
            <span class="bulk-actions-count">
                {move || {
                    let count = selected.get().len();
                    format!("{count} selected")
                }}
            </span>
            <select
                class="folder-select"
                on:change=move |ev| {
                    let value = event_target_value(&ev);
                    let folder_id = (!value.is_empty()).then_some(value);
                    for id in selected.get_untracked() {
                        actions
                            .move_file
                            .dispatch(MoveFileToFolder { id, folder_id: folder_id.clone() });
                    }
                    clear();
                }
            >
                <option value="">"no folder"</option>
                {folders
                    .into_iter()
                    .map(|folder| {
                        view! { <option value=folder.id.clone()>{folder.name.clone()}</option> }
                    })
                    .collect_view()}
            </select>
            <button
                type="button"
                class="btn btn-ghost"
                on:click=move |_| {
                    for id in selected.get_untracked() {
                        actions.delete_file.dispatch(DeleteFile { id });
                    }
                    clear();
                }
            >
                "delete selected"
            </button>
            <button type="button" class="btn btn-ghost" on:click=move |_| clear()>
                "cancel"
            </button>
        </div>
    }
}

#[component]
fn FileCard(
    file: FileSummary,
    actions: Actions,
    opened_file: RwSignal<Option<String>>,
    search: RwSignal<String>,
    select_mode: RwSignal<bool>,
    selected: RwSignal<std::collections::HashSet<String>>,
) -> impl IntoView {
    let (copied, set_copied) = signal(false);
    let (thumb_failed, set_thumb_failed) = signal(false);

    let is_image = file.content_type.starts_with("image/");
    let copy_url = file.url.clone();
    let thumbnail_url = file.thumbnail_url.clone();
    let id_for_delete = file.id.clone();
    let id_for_click = file.id.clone();
    let id_for_toggle = file.id.clone();
    let id_for_card_class = file.id.clone();
    let id_for_badge = file.id.clone();
    let name = file.original_name.clone();
    let name_for_alt = name.clone();
    let content_type = file.content_type.clone();
    let size = file.size_bytes;
    let date = format_date(&file.created_at).to_string();
    let has_password = file.has_password;
    let has_expiry = file.expires_at.is_some();
    let tags = file.tags.clone();

    let copy = move |_| {
        copy_to_clipboard(&copy_url);
        set_copied.set(true);
        set_timeout(
            move || set_copied.set(false),
            std::time::Duration::from_millis(1500),
        );
    };

    view! {
        <div
            class="file-card"
            class:selected=move || selected.get().contains(&id_for_card_class)
        >
            <div class="file-thumb">
                <button
                    type="button"
                    class="file-thumb-btn"
                    title="view details"
                    on:click=move |_| {
                        if select_mode.get() {
                            let id = id_for_toggle.clone();
                            selected
                                .update(|set| {
                                    if !set.insert(id.clone()) {
                                        set.remove(&id);
                                    }
                                });
                        } else {
                            opened_file.set(Some(id_for_click.clone()));
                        }
                    }
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
                {move || {
                    select_mode
                        .get()
                        .then(|| {
                            let checked = selected.get().contains(&id_for_badge);
                            view! {
                                <div class="select-badge" class:checked=checked>
                                    {checked.then(|| view! { <CheckIcon/> })}
                                </div>
                            }
                        })
                }}
                {has_password.then(|| view! { <div class="lock-badge"><LockIcon/></div> })}
                {has_expiry
                    .then(|| {
                        view! {
                            <div class="expiry-badge" title="expires">
                                <ClockIcon/>
                            </div>
                        }
                    })}
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
                {(!tags.is_empty())
                    .then(|| {
                        view! {
                            <div class="tag-list">
                                {tags
                                    .into_iter()
                                    .map(|tag| {
                                        let tag_for_click = tag.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="tag-chip"
                                                title="search this tag"
                                                on:click=move |_| search.set(tag_for_click.clone())
                                            >
                                                {tag}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        }
                    })}
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
    let (renaming, set_renaming) = signal(false);
    let (password_input, set_password_input) = signal(String::new());
    let (editing_password, set_editing_password) = signal(false);
    let (name_input, set_name_input) = signal(file.original_name.clone());
    let (editing_expiry, set_editing_expiry) = signal(false);
    let (expiry_preset_input, set_expiry_preset_input) = signal("never".to_string());
    let (editing_tags, set_editing_tags) = signal(false);
    let (tags_input, set_tags_input) = signal(file.tags.join(", "));

    let is_image = file.content_type.starts_with("image/");
    let is_video = file.content_type.starts_with("video/");
    let is_audio = file.content_type.starts_with("audio/");

    let copy_url = file.url.clone();
    let id_for_delete = file.id.clone();
    let id_for_move = file.id.clone();
    let id_for_password = file.id.clone();
    let id_for_rename = file.id.clone();
    let id_for_expiry = file.id.clone();
    let id_for_tags = file.id.clone();
    let current_folder = file.folder_id.clone();
    let has_password = file.has_password;
    let has_expiry = file.expires_at.is_some();
    let display_name = file.original_name.clone();
    let name_for_cancel = file.original_name.clone();
    let expires_display = file
        .expires_at
        .as_deref()
        .map(format_date)
        .unwrap_or("never")
        .to_string();
    let tags_for_display = file.tags.clone();
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
                    <div class="modal-title-row">
                        {move || {
                            let name_for_cancel = name_for_cancel.clone();
                            let id_for_rename = id_for_rename.clone();
                            if renaming.get() {
                                view! {
                                    <form
                                        class="modal-rename"
                                        on:submit=move |ev| {
                                            ev.prevent_default();
                                            let name = name_input.get();
                                            if !name.trim().is_empty() {
                                                actions
                                                    .rename_file
                                                    .dispatch(RenameFile {
                                                        id: id_for_rename.clone(),
                                                        name,
                                                    });
                                            }
                                            set_renaming.set(false);
                                        }
                                    >
                                        <input
                                            class="modal-title-input"
                                            type="text"
                                            prop:value=move || name_input.get()
                                            on:input=move |ev| {
                                                set_name_input.set(event_target_value(&ev))
                                            }
                                        />
                                        <button type="submit" class="btn btn-ghost">
                                            "save"
                                        </button>
                                        <button
                                            type="button"
                                            class="btn btn-ghost"
                                            on:click=move |_| {
                                                set_name_input.set(name_for_cancel.clone());
                                                set_renaming.set(false);
                                            }
                                        >
                                            "cancel"
                                        </button>
                                    </form>
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <h3 class="modal-title" title=display_name.clone()>
                                        {display_name.clone()}
                                    </h3>
                                }
                                    .into_any()
                            }
                        }}
                    </div>

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
                            <dt>"expires"</dt>
                            <dd>{expires_display}</dd>
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

                    {(!tags_for_display.is_empty())
                        .then(|| {
                            view! {
                                <div class="tag-list">
                                    {tags_for_display
                                        .iter()
                                        .map(|tag| view! { <span class="tag-chip">{tag.clone()}</span> })
                                        .collect_view()}
                                </div>
                            }
                        })}

                    {move || {
                        let id_for_tags = id_for_tags.clone();
                        editing_tags
                            .get()
                            .then(|| {
                                view! {
                                    <form
                                        class="password-inline"
                                        on:submit=move |ev| {
                                            ev.prevent_default();
                                            let tags = tags_input
                                                .get()
                                                .split(',')
                                                .map(|tag| tag.trim().to_string())
                                                .filter(|tag| !tag.is_empty())
                                                .collect();
                                            actions
                                                .file_tags
                                                .dispatch(SetFileTags {
                                                    id: id_for_tags.clone(),
                                                    tags,
                                                });
                                            set_editing_tags.set(false);
                                        }
                                    >
                                        <input
                                            type="text"
                                            placeholder="comma-separated tags"
                                            prop:value=move || tags_input.get()
                                            on:input=move |ev| {
                                                set_tags_input.set(event_target_value(&ev))
                                            }
                                        />
                                        <button type="submit" class="btn btn-ghost">
                                            "save"
                                        </button>
                                        <button
                                            type="button"
                                            class="btn btn-ghost"
                                            on:click=move |_| set_editing_tags.set(false)
                                        >
                                            "cancel"
                                        </button>
                                    </form>
                                }
                            })
                    }}

                    {move || {
                        let id_for_expiry = id_for_expiry.clone();
                        editing_expiry
                            .get()
                            .then(|| {
                                view! {
                                    <form
                                        class="password-inline"
                                        on:submit=move |ev| {
                                            ev.prevent_default();
                                            actions
                                                .file_expiry
                                                .dispatch(SetFileExpiry {
                                                    id: id_for_expiry.clone(),
                                                    preset: expiry_preset_input.get(),
                                                });
                                            set_editing_expiry.set(false);
                                        }
                                    >
                                        <select
                                            class="folder-select"
                                            on:change=move |ev| {
                                                set_expiry_preset_input.set(event_target_value(&ev))
                                            }
                                        >
                                            {EXPIRY_PRESETS
                                                .iter()
                                                .map(|(value, label)| {
                                                    let value = value.to_string();
                                                    let value_for_select = value.clone();
                                                    let selected = move || {
                                                        expiry_preset_input.get() == value_for_select
                                                    };
                                                    view! {
                                                        <option value=value selected=selected>
                                                            {*label}
                                                        </option>
                                                    }
                                                })
                                                .collect_view()}
                                        </select>
                                        <button type="submit" class="btn btn-ghost">
                                            "save"
                                        </button>
                                        <button
                                            type="button"
                                            class="btn btn-ghost"
                                            on:click=move |_| set_editing_expiry.set(false)
                                        >
                                            "cancel"
                                        </button>
                                    </form>
                                }
                            })
                    }}

                    {move || {
                        let id_for_password = id_for_password.clone();
                        editing_password
                            .get()
                            .then(|| {
                                view! {
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
                                            set_editing_password.set(false);
                                        }
                                    >
                                        <input
                                            type="password"
                                            placeholder=if has_password {
                                                "change or clear password"
                                            } else {
                                                "set a password"
                                            }
                                            on:input=move |ev| {
                                                set_password_input.set(event_target_value(&ev))
                                            }
                                        />
                                        <button type="submit" class="btn btn-ghost">
                                            "save"
                                        </button>
                                        <button
                                            type="button"
                                            class="btn btn-ghost"
                                            on:click=move |_| {
                                                set_password_input.set(String::new());
                                                set_editing_password.set(false);
                                            }
                                        >
                                            "cancel"
                                        </button>
                                    </form>
                                }
                            })
                    }}

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
                            type="button"
                            class="icon-btn"
                            title="edit name"
                            on:click=move |_| set_renaming.set(true)
                        >
                            <PencilIcon/>
                        </button>
                        <button
                            type="button"
                            class="icon-btn"
                            title="edit tags"
                            on:click=move |_| set_editing_tags.set(true)
                        >
                            <TagIcon/>
                        </button>
                        <button
                            type="button"
                            class="icon-btn"
                            title=if has_expiry { "change expiry" } else { "set an expiry" }
                            on:click=move |_| set_editing_expiry.set(true)
                        >
                            <ClockIcon/>
                        </button>
                        <button
                            type="button"
                            class="icon-btn"
                            title=if has_password { "change password" } else { "set a password" }
                            on:click=move |_| set_editing_password.set(true)
                        >
                            <LockIcon/>
                        </button>
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
