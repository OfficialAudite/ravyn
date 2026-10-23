mod account;
mod files;
mod folders;
mod shorten;
mod uploads;
mod view;

pub use account::run_session_sweep;
pub use files::run_expiry_sweep;
pub use uploads::run_chunked_upload_sweep;

use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, patch, post, put},
    Form, Router,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use ravyn_core::{auth as core_auth, File, User, UserId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{rate_limit::client_ip, state::AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/files/{id}", get(files::get_file))
        .route("/files/{id}", delete(files::delete_file))
        .route("/files/{id}", post(unlock_file))
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
        .route("/admin/users/{id}", delete(account::delete_user))
        .route("/admin/stats", get(account::admin_stats))
        .route("/admin/activity", get(account::list_activity))
        .route(
            "/admin/cost-settings",
            get(account::get_cost_settings).put(account::set_cost_settings),
        )
        .route(
            "/admin/compression-settings",
            get(account::get_compression_settings).put(account::set_compression_settings),
        )
        .route("/v/{id}", get(view::view_file).post(unlock_file))
        .route("/short-urls", post(shorten::create_short_url))
        .route("/short-urls", get(shorten::list_short_urls))
        .route("/short-urls/{id}", delete(shorten::delete_short_url))
        .route("/s/{slug}", get(shorten::redirect_short_url))
        .route("/uploads", post(uploads::init_chunked_upload))
        .route("/uploads/{id}", get(uploads::get_chunked_upload_status))
        .route("/uploads/{id}", delete(uploads::cancel_chunked_upload))
        .route(
            "/uploads/{id}/complete",
            post(uploads::complete_chunked_upload),
        )
        .route("/uploads/{id}/{part_number}", patch(uploads::upload_chunk))
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

/// A readiness check, not just a liveness one: confirms the process can
/// still round-trip a real query against Postgres, not just that it's
/// running. Bounded by a short timeout of its own so a stuck pool
/// (exactly the class of bug `Db::connect`'s own tuning guards against)
/// makes this fail fast instead of hanging right along with everything
/// else - the whole point of a health check a monitor can act on.
async fn health(State(state): State<AppState>) -> Response {
    match tokio::time::timeout(std::time::Duration::from_secs(3), state.db.ping()).await {
        Ok(Ok(())) => "ok".into_response(),
        Ok(Err(err)) => {
            tracing::error!(%err, "health check: database ping failed");
            (StatusCode::SERVICE_UNAVAILABLE, "database unreachable").into_response()
        }
        Err(_) => {
            tracing::error!("health check: database ping timed out");
            (StatusCode::SERVICE_UNAVAILABLE, "database timed out").into_response()
        }
    }
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

pub enum Access {
    Granted,
    Denied,
    Throttled,
}

/// The owner always gets in, anyone else needs the password if one is set.
/// Only wrong guesses count against the limiter, so someone streaming a
/// protected video (one request per seek) never trips it.
pub async fn check_access(
    state: &AppState,
    headers: &HeaderMap,
    resource: Uuid,
    owner_id: UserId,
    password_hash: Option<&str>,
    requester: Option<&User>,
    password: Option<&str>,
) -> Access {
    if requester.is_some_and(|user| user.id == owner_id) {
        return Access::Granted;
    }
    let Some(hash) = password_hash else {
        return Access::Granted;
    };
    let unlocked = CookieJar::from_headers(headers)
        .get(&unlock_cookie_name(resource))
        .is_some_and(|cookie| cookie.value() == unlock_token(resource, hash));
    if unlocked {
        return Access::Granted;
    }
    let Some(candidate) = password else {
        return Access::Denied;
    };

    let limiter = &state.rate_limiters.resource_password;
    let key = format!("{}:{resource}", client_ip(headers));
    if limiter.is_blocked(&key) {
        return Access::Throttled;
    }
    if crate::auth::verify_password(candidate, hash).await {
        Access::Granted
    } else {
        limiter.check(&key);
        Access::Denied
    }
}

fn unlock_cookie_name(resource: Uuid) -> String {
    format!("ravyn_unlock_{}", resource.simple())
}

/// Derived from the password hash, so changing or removing the password
/// invalidates every unlock cookie handed out for the old one.
fn unlock_token(resource: Uuid, password_hash: &str) -> String {
    core_auth::hash_token(&format!("unlock:{resource}:{password_hash}"))
}

pub fn cookie_secure(state: &AppState) -> bool {
    state
        .public_url
        .as_deref()
        .is_some_and(|url| url.starts_with("https://"))
}

#[derive(Deserialize)]
pub struct UnlockForm {
    password: String,
}

/// The password prompt posts here, so the password never ends up in a URL
/// (browser history, server logs, `Referer`). A correct one sets a cookie
/// for this file and redirects back to the same, clean URL.
pub async fn unlock_file(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    uri: Uri,
    headers: HeaderMap,
    Form(form): Form<UnlockForm>,
) -> Response {
    let file = match state.db.get_file(ravyn_core::FileId(id)).await {
        Ok(Some(file)) => file,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let Some(hash) = file.password_hash.as_deref() else {
        return Redirect::to(uri.path()).into_response();
    };

    let access = check_access(
        &state,
        &headers,
        id,
        file.owner_id,
        Some(hash),
        None,
        Some(&form.password),
    )
    .await;
    match access {
        Access::Granted => {
            let cookie = Cookie::build((unlock_cookie_name(id), unlock_token(id, hash)))
                .path("/")
                .http_only(true)
                .secure(cookie_secure(&state))
                .same_site(SameSite::Lax)
                .max_age(time::Duration::days(1))
                .build();
            (CookieJar::new().add(cookie), Redirect::to(uri.path())).into_response()
        }
        Access::Throttled => too_many_password_attempts(),
        Access::Denied => (
            StatusCode::UNAUTHORIZED,
            Html(password_prompt_html("wrong password")),
        )
            .into_response(),
    }
}

pub fn too_many_password_attempts() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        "too many wrong passwords, try again later",
    )
        .into_response()
}

/// Hashes a password for storage, treating an empty string the same as
/// "no password" (clears it). Shared by files' and folders' password
/// endpoints.
pub async fn hash_optional_password(
    password: Option<String>,
) -> Result<Option<String>, StatusCode> {
    match password.filter(|p| !p.is_empty()) {
        Some(password) => crate::auth::hash_password(&password)
            .await
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
  <form method="post">
    <p>{message}</p>
    <input type="password" name="password" placeholder="password" autofocus>
    <button type="submit">unlock</button>
  </form>
</body></html>"#
    )
}
