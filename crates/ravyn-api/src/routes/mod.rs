mod account;
mod files;
mod folders;
mod shorten;
mod view;

pub use files::run_expiry_sweep;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderMap, StatusCode},
    routing::{delete, get, post, put},
    Router,
};
use ravyn_core::{auth as core_auth, File, User};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/files/{id}", get(files::get_file))
        .route("/files/{id}", delete(files::delete_file))
        .route("/files/{id}/thumbnail", get(files::get_file_thumbnail))
        .route("/files/{id}/folder", put(files::set_file_folder))
        .route("/files/{id}/password", put(files::set_file_password))
        .route("/files/{id}/name", put(files::set_file_name))
        .route("/files/{id}/expiry", put(files::set_file_expiry))
        .route("/files/{id}/tags", put(files::set_file_tags))
        .route("/files", get(files::list_files))
        .route("/files", post(files::upload_file))
        .route("/folders", post(folders::create_folder))
        .route("/folders", get(folders::list_folders))
        .route("/folders/{id}", delete(folders::delete_folder))
        .route("/folders/{id}/password", put(folders::set_folder_password))
        .route("/folders/{id}/files", get(folders::get_folder_files))
        .route("/login", post(account::login))
        .route("/login/totp", post(account::login_totp))
        .route("/logout", post(account::logout))
        .route("/register", post(account::register))
        .route("/registration-status", get(account::registration_status))
        .route("/me", get(account::me))
        .route("/me/password", put(account::change_password))
        .route("/me/totp/setup", post(account::setup_totp))
        .route("/me/totp/confirm", post(account::confirm_totp))
        .route("/me/totp/disable", post(account::disable_totp))
        .route("/me/stats", get(account::my_stats))
        .route("/api-tokens", post(account::create_api_token))
        .route("/api-tokens", get(account::list_api_tokens))
        .route("/api-tokens/{id}", delete(account::delete_api_token))
        .route("/storage-info", get(account::storage_info))
        .route("/embed-settings", get(account::get_embed_settings))
        .route("/embed-settings", put(account::set_embed_settings))
        .route("/webhook-settings", get(account::get_webhook_settings))
        .route("/webhook-settings", put(account::set_webhook_settings))
        .route("/instance-settings", get(account::get_instance_settings))
        .route("/instance-settings", put(account::set_instance_settings))
        .route("/invites", post(account::create_invite))
        .route("/invites", get(account::list_invites))
        .route("/invites/{id}", delete(account::delete_invite))
        .route("/admin/users", get(account::list_users))
        .route("/admin/users/{id}/limit", put(account::set_user_limit))
        .route("/admin/stats", get(account::admin_stats))
        .route("/v/{id}", get(view::view_file))
        .route("/short-urls", post(shorten::create_short_url))
        .route("/short-urls", get(shorten::list_short_urls))
        .route("/short-urls/{id}", delete(shorten::delete_short_url))
        .route("/s/{slug}", get(shorten::redirect_short_url))
        .layer(DefaultBodyLimit::max(max_upload_bytes()))
        .with_state(state)
}

/// Axum's own default (2 MB, meant for JSON APIs, not file uploads) is
/// nowhere near enough for this app — `MAX_UPLOAD_MB` overrides it, in
/// megabytes, on both this server and `ravyn-web` (which needs its own copy
/// of this same limit, since `/upload` on `ravyn-web` receives the whole
/// multipart body itself before forwarding it here). The upload path
/// buffers each part fully in memory before writing it to storage — a very
/// large default trades away that safety margin, so this stays generous
/// rather than unbounded.
fn max_upload_bytes() -> usize {
    std::env::var("MAX_UPLOAD_MB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2048)
        .saturating_mul(1024 * 1024)
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
pub struct FileSummary {
    pub id: Uuid,
    pub original_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub folder_id: Option<Uuid>,
    pub has_password: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    pub tags: Vec<String>,
}

impl From<File> for FileSummary {
    fn from(file: File) -> Self {
        FileSummary {
            id: file.id.0,
            original_name: file.original_name,
            content_type: file.content_type,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
            folder_id: file.folder_id.map(|id| id.0),
            has_password: file.password_hash.is_some(),
            created_at: file.created_at,
            expires_at: file.expires_at,
            tags: file.tags,
        }
    }
}

/// A file is viewable without a password by its owner, or by anyone who
/// either doesn't need one (none set) or supplied the right one.
pub fn is_authorized(file: &File, requester: Option<&User>, password: Option<&str>) -> bool {
    if let Some(user) = requester {
        if user.id == file.owner_id {
            return true;
        }
    }

    match &file.password_hash {
        None => true,
        Some(hash) => password
            .map(|candidate| core_auth::verify_password(candidate, hash))
            .unwrap_or(false),
    }
}

/// Hashes a password for storage, treating an empty string the same as
/// "no password" (clears it). Shared by files' and folders' password
/// endpoints.
pub fn hash_optional_password(password: Option<String>) -> Result<Option<String>, StatusCode> {
    match password.filter(|p| !p.is_empty()) {
        Some(password) => core_auth::hash_password(&password)
            .map(Some)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR),
        None => Ok(None),
    }
}

/// The origin this instance is reachable at, purely as far as *this
/// request's own headers* say — accurate when the browser hit `ravyn-api`
/// directly (a `/v/{id}` link, ShareX), but not when this request actually
/// arrived via `ravyn-web`'s `/upload` proxy: that hop's own outgoing
/// request carries `ravyn-web`'s address, not the browser's. Use
/// `resolve_public_base_url` instead unless you specifically know this
/// request was never proxied.
fn public_base_url(headers: &HeaderMap) -> String {
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    format!("{proto}://{host}")
}

/// The origin to use for a link that gets embedded somewhere else (an OG
/// tag, a webhook message) rather than just followed by whoever's already
/// here. Prefers `RAVYN_PUBLIC_API_URL` (set once, by the operator — see
/// `docker-compose.yml`) over anything derived from this request's own
/// headers, since a request proxied through `ravyn-web`'s `/upload` has no
/// reliable way to know the browser's real address (see
/// `public_base_url`'s doc comment). Falls back to header-derived only when
/// that env var isn't set, which is still correct for the common
/// direct-to-`ravyn-api` case.
pub fn resolve_public_base_url(state: &AppState, headers: &HeaderMap) -> String {
    state
        .public_url
        .clone()
        .unwrap_or_else(|| public_base_url(headers))
}

/// A minimal, self-contained password prompt for a directly-shared link.
/// Deliberately plain HTML/inline CSS rather than pulling in a templating
/// engine — `ravyn-api` stays a pure JSON+bytes service everywhere else.
pub fn password_prompt_html(message: &str) -> String {
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>password required</title>
<style>
  body {{ margin:0; min-height:100vh; display:grid; place-items:center; background:#0c0d12;
         color:#eeecf5; font-family: ui-sans-serif, system-ui, sans-serif; }}
  form {{ background:#15161d; border:1px solid #282a35; border-radius:14px; padding:2rem;
          text-align:center; width:280px; }}
  input {{ width:100%; box-sizing:border-box; background:#0c0d12; border:1px solid #282a35;
           border-radius:8px; padding:.6rem .75rem; margin:.75rem 0; color:inherit; font-size:.9rem; }}
  button {{ width:100%; border:none; border-radius:8px; padding:.6rem; font-weight:600; cursor:pointer;
            background:linear-gradient(115deg,#5b8dff,#b26bff 55%,#3ee6c4); }}
  p {{ color:#9291a3; font-size:.8rem; margin:0 0 .5rem; }}
</style></head>
<body>
  <form method="get">
    <p>{message}</p>
    <input type="password" name="password" placeholder="password" autofocus>
    <button type="submit">unlock</button>
  </form>
</body></html>"#
    )
}
