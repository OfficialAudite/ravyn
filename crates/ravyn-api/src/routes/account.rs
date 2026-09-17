use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use ravyn_core::{
    auth as core_auth, ApiToken, ApiTokenId, EmbedSettings, ExpiryPreset, Invite, InviteId,
    NamingScheme, RegistrationMode, User, UserId,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    auth::{AuthedUser, SESSION_COOKIE, SESSION_LIFETIME},
    state::AppState,
};

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

pub async fn login(State(state): State<AppState>, Json(body): Json<LoginRequest>) -> Response {
    let user = match state.db.get_user_by_username(&body.username).await {
        Ok(Some(user)) => user,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up user");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if !core_auth::verify_password(&body.password, &user.password_hash) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    new_session_response(&state, user.id).await
}

/// Creates a session for `user_id` and sets its cookie — shared by `login`
/// and `register`, since registering also signs you straight in.
async fn new_session_response(state: &AppState, user_id: UserId) -> Response {
    let token = core_auth::generate_token();
    let session = ravyn_core::Session {
        token_hash: core_auth::hash_token(&token),
        user_id,
        expires_at: OffsetDateTime::now_utc() + SESSION_LIFETIME,
        created_at: OffsetDateTime::now_utc(),
    };

    if let Err(err) = state.db.create_session(&session).await {
        tracing::error!(%err, "failed to create session");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // `.secure(true)` should be set once this sits behind a TLS-terminating
    // reverse proxy; left off here so local `http://` development still works.
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(SESSION_LIFETIME)
        .build();

    (CookieJar::new().add(cookie), StatusCode::OK).into_response()
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    username: String,
    password: String,
    invite_token: Option<String>,
}

/// The very first account on an instance always gets through here and
/// becomes an admin — there'd be no admin able to open registration up
/// otherwise. Every account after that is gated by whatever registration
/// mode the admin has set (`RegistrationMode`), checked fresh on every
/// call rather than cached anywhere.
pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Response {
    if body.username.trim().is_empty() || body.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "username and password are required",
        )
            .into_response();
    }

    let is_first_user = match state.db.has_any_users().await {
        Ok(any) => !any,
        Err(err) => {
            tracing::error!(%err, "failed to check for existing users");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut redeemed_invite = None;

    if !is_first_user {
        let mode = match state.db.get_registration_mode().await {
            Ok(mode) => mode,
            Err(err) => {
                tracing::error!(%err, "failed to load registration mode");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

        match mode {
            RegistrationMode::Closed => return StatusCode::FORBIDDEN.into_response(),
            RegistrationMode::Open => {}
            RegistrationMode::Invite => {
                let Some(token) = body.invite_token.as_deref().filter(|t| !t.is_empty()) else {
                    return (StatusCode::BAD_REQUEST, "an invite code is required").into_response();
                };

                let invite = match state
                    .db
                    .get_unused_invite_by_token_hash(&core_auth::hash_token(token))
                    .await
                {
                    Ok(Some(invite)) => invite,
                    Ok(None) => {
                        return (StatusCode::FORBIDDEN, "invalid or already-used invite code")
                            .into_response()
                    }
                    Err(err) => {
                        tracing::error!(%err, "failed to look up invite");
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                };
                redeemed_invite = Some(invite.id);
            }
        }
    }

    match state.db.get_user_by_username(&body.username).await {
        Ok(Some(_)) => return (StatusCode::CONFLICT, "username already taken").into_response(),
        Ok(None) => {}
        Err(err) => {
            tracing::error!(%err, "failed to look up user");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    let password_hash = match core_auth::hash_password(&body.password) {
        Ok(hash) => hash,
        Err(err) => {
            tracing::error!(%err, "failed to hash password");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let user = User {
        id: UserId::new(),
        username: body.username,
        password_hash,
        is_admin: is_first_user,
        created_at: OffsetDateTime::now_utc(),
    };

    if let Err(err) = state.db.create_user(&user).await {
        tracing::error!(%err, "failed to create user");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if let Some(invite_id) = redeemed_invite {
        if let Err(err) = state.db.mark_invite_used(invite_id, user.id).await {
            tracing::warn!(%err, "failed to mark invite as used");
        }
    }

    new_session_response(&state, user.id).await
}

/// Public: lets the login screen decide whether to offer "create an
/// account" (and what it needs to ask for) without requiring auth to find
/// out — there's nothing sensitive in "is this instance claimed yet" or
/// "what's the current registration mode".
pub async fn registration_status(State(state): State<AppState>) -> Response {
    let setup_required = match state.db.has_any_users().await {
        Ok(any) => !any,
        Err(err) => {
            tracing::error!(%err, "failed to check for existing users");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mode = match state.db.get_registration_mode().await {
        Ok(mode) => mode,
        Err(err) => {
            tracing::error!(%err, "failed to load registration mode");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(serde_json::json!({ "setup_required": setup_required, "mode": mode.as_str() }))
        .into_response()
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        let token_hash = core_auth::hash_token(cookie.value());
        let _ = state.db.delete_session(&token_hash).await;
    }

    (jar.remove(Cookie::from(SESSION_COOKIE)), StatusCode::OK).into_response()
}

pub async fn me(AuthedUser(user): AuthedUser) -> Response {
    Json(serde_json::json!({ "username": user.username, "is_admin": user.is_admin }))
        .into_response()
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// Requires the current password rather than trusting the session alone —
/// the same reasoning most services have for this, since a session cookie
/// can outlive the moment someone meant to be signed in (a shared machine,
/// a stolen cookie) in a way a freshly-typed password can't.
pub async fn change_password(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<ChangePasswordRequest>,
) -> Response {
    if !core_auth::verify_password(&body.current_password, &user.password_hash) {
        return (StatusCode::UNAUTHORIZED, "current password is incorrect").into_response();
    }

    if body.new_password.is_empty() {
        return (StatusCode::BAD_REQUEST, "new password is required").into_response();
    }

    let password_hash = match core_auth::hash_password(&body.new_password) {
        Ok(hash) => hash,
        Err(err) => {
            tracing::error!(%err, "failed to hash password");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Err(err) = state.db.set_password_hash(user.id, password_hash).await {
        tracing::error!(%err, "failed to save new password");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Every field optional so the registration form and the naming-scheme form
/// (two independent forms on the same admin page) can each save just their
/// own settings without clobbering the other's.
#[derive(Deserialize)]
pub struct SetInstanceSettingsRequest {
    registration_mode: Option<String>,
    naming_scheme: Option<String>,
    random_name_length: Option<i64>,
    default_expiry_preset: Option<String>,
    strip_exif: Option<bool>,
}

pub async fn get_instance_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mode = match state.db.get_registration_mode().await {
        Ok(mode) => mode,
        Err(err) => {
            tracing::error!(%err, "failed to load registration mode");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let naming_scheme = match state.db.get_naming_scheme().await {
        Ok(scheme) => scheme,
        Err(err) => {
            tracing::error!(%err, "failed to load naming scheme");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let random_name_length = state.db.get_random_name_length().await.unwrap_or(8);
    let default_expiry_preset = state
        .db
        .get_default_expiry_preset()
        .await
        .unwrap_or(ExpiryPreset::Never);
    let strip_exif = state.db.get_strip_exif().await.unwrap_or(true);

    Json(serde_json::json!({
        "registration_mode": mode.as_str(),
        "naming_scheme": naming_scheme.as_str(),
        "random_name_length": random_name_length,
        "default_expiry_preset": default_expiry_preset.as_str(),
        "strip_exif": strip_exif,
    }))
    .into_response()
}

/// Clamped rather than rejected: a stray very-short or very-long value from
/// a hand-edited request is a nuisance, not something worth failing an
/// otherwise-valid save over.
const MIN_RANDOM_NAME_LENGTH: i64 = 4;
const MAX_RANDOM_NAME_LENGTH: i64 = 64;

pub async fn set_instance_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<SetInstanceSettingsRequest>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Some(raw_mode) = body.registration_mode {
        let Some(mode) = RegistrationMode::parse(&raw_mode) else {
            return (StatusCode::BAD_REQUEST, "invalid registration mode").into_response();
        };
        if let Err(err) = state.db.set_registration_mode(mode).await {
            tracing::error!(%err, "failed to save registration mode");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Some(raw_scheme) = body.naming_scheme {
        let Some(scheme) = NamingScheme::parse(&raw_scheme) else {
            return (StatusCode::BAD_REQUEST, "invalid naming scheme").into_response();
        };
        if let Err(err) = state.db.set_naming_scheme(scheme).await {
            tracing::error!(%err, "failed to save naming scheme");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Some(length) = body.random_name_length {
        let length = length.clamp(MIN_RANDOM_NAME_LENGTH, MAX_RANDOM_NAME_LENGTH);
        if let Err(err) = state.db.set_random_name_length(length).await {
            tracing::error!(%err, "failed to save random name length");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Some(raw_preset) = body.default_expiry_preset {
        let Some(preset) = ExpiryPreset::parse(&raw_preset) else {
            return (StatusCode::BAD_REQUEST, "invalid expiry preset").into_response();
        };
        if let Err(err) = state.db.set_default_expiry_preset(preset).await {
            tracing::error!(%err, "failed to save default expiry preset");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Some(strip_exif) = body.strip_exif {
        if let Err(err) = state.db.set_strip_exif(strip_exif).await {
            tracing::error!(%err, "failed to save strip-exif setting");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Mints a single-use invite code for invite-only registration. The
/// plaintext code is only ever returned here, once — same pattern as an API
/// token.
pub async fn create_invite(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let token = core_auth::generate_token();
    let invite = Invite {
        id: InviteId::new(),
        token_hash: core_auth::hash_token(&token),
        created_by: user.id,
        created_at: OffsetDateTime::now_utc(),
        used_by: None,
        used_at: None,
    };

    if let Err(err) = state.db.create_invite(&invite).await {
        tracing::error!(%err, "failed to create invite");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(serde_json::json!({ "id": invite.id.0, "token": token })).into_response()
}

#[derive(Serialize)]
pub struct InviteSummary {
    id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    used: bool,
}

impl From<Invite> for InviteSummary {
    fn from(invite: Invite) -> Self {
        InviteSummary {
            id: invite.id.0,
            created_at: invite.created_at,
            used: invite.used_at.is_some(),
        }
    }
}

pub async fn list_invites(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    match state.db.list_invites().await {
        Ok(invites) => {
            let summaries: Vec<InviteSummary> =
                invites.into_iter().map(InviteSummary::from).collect();
            Json(summaries).into_response()
        }
        Err(err) => {
            tracing::error!(%err, "failed to list invites");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn delete_invite(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(err) = state.db.delete_invite(InviteId(id)).await {
        tracing::error!(%err, "failed to delete invite");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Serialize)]
pub struct AdminUserSummary {
    id: Uuid,
    username: String,
    is_admin: bool,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    storage_used_bytes: i64,
    max_storage_bytes: Option<i64>,
    file_count: i64,
}

/// Every account on the instance with its current storage usage — an
/// admin's view of who's using what, and the basis for setting limits.
/// Usage is computed per user rather than fetched in one join here; at the
/// scale a self-hosted instance runs at, N+1 simple indexed queries costs
/// nothing and keeps this handler from having to know how that join works.
pub async fn list_users(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let users = match state.db.list_users().await {
        Ok(users) => users,
        Err(err) => {
            tracing::error!(%err, "failed to list users");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut summaries = Vec::with_capacity(users.len());
    for u in users {
        let storage_used_bytes = state.db.get_storage_usage(u.id).await.unwrap_or(0);
        let max_storage_bytes = state.db.get_max_storage_bytes(u.id).await.unwrap_or(None);
        let file_count = state.db.count_files_for_owner(u.id).await.unwrap_or(0);
        summaries.push(AdminUserSummary {
            id: u.id.0,
            username: u.username,
            is_admin: u.is_admin,
            created_at: u.created_at,
            storage_used_bytes,
            max_storage_bytes,
            file_count,
        });
    }

    Json(summaries).into_response()
}

#[derive(Deserialize)]
pub struct SetUserLimitRequest {
    max_storage_bytes: Option<i64>,
}

pub async fn set_user_limit(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetUserLimitRequest>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(err) = state
        .db
        .set_max_storage_bytes(UserId(id), body.max_storage_bytes)
        .await
    {
        tracing::error!(%err, "failed to set storage limit");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Buckets a content type the same way the browse page's type filter does
/// (`TypeFilter` in `crates/ravyn-web/src/dashboard.rs`) — kept as a small
/// duplicate here rather than a shared crate, since it's four lines and the
/// two call sites (a filter predicate, a stats tally) want it in different
/// shapes anyway.
#[derive(Default, Serialize)]
struct TypeCounts {
    images: i64,
    videos: i64,
    audio: i64,
    documents: i64,
    other: i64,
}

impl TypeCounts {
    fn add(&mut self, content_type: &str) {
        if content_type.starts_with("image/") {
            self.images += 1;
        } else if content_type.starts_with("video/") {
            self.videos += 1;
        } else if content_type.starts_with("audio/") {
            self.audio += 1;
        } else if content_type == "application/pdf" || content_type.starts_with("text/") {
            self.documents += 1;
        } else {
            self.other += 1;
        }
    }
}

#[derive(Serialize)]
pub struct MyStats {
    file_count: i64,
    storage_used_bytes: i64,
    max_storage_bytes: Option<i64>,
    #[serde(flatten)]
    by_type: TypeCounts,
}

/// Every user's own view of their usage — unlike `list_users`, open to
/// anyone, not just an admin.
pub async fn my_stats(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    let files = match state.db.list_files_for_owner(user.id).await {
        Ok(files) => files,
        Err(err) => {
            tracing::error!(%err, "failed to list files for stats");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let max_storage_bytes = state
        .db
        .get_max_storage_bytes(user.id)
        .await
        .unwrap_or(None);

    let mut by_type = TypeCounts::default();
    let mut storage_used_bytes = 0i64;
    for file in &files {
        storage_used_bytes += file.size_bytes as i64;
        by_type.add(&file.content_type);
    }

    Json(MyStats {
        file_count: files.len() as i64,
        storage_used_bytes,
        max_storage_bytes,
        by_type,
    })
    .into_response()
}

#[derive(Serialize)]
pub struct InstanceStats {
    total_users: i64,
    total_files: i64,
    total_storage_bytes: i64,
    #[serde(flatten)]
    by_type: TypeCounts,
}

/// The instance-wide counterpart to `my_stats` — every user's usage summed
/// together, admin only. Reuses `list_files_for_owner` per user rather than
/// a new "every file regardless of owner" query, for the same reason
/// `list_users` already does N+1 queries for the per-user list: at
/// self-hosted scale it's simpler than it is slow.
pub async fn admin_stats(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let users = match state.db.list_users().await {
        Ok(users) => users,
        Err(err) => {
            tracing::error!(%err, "failed to list users for stats");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut by_type = TypeCounts::default();
    let mut total_files = 0i64;
    let mut total_storage_bytes = 0i64;

    for u in &users {
        let files = state
            .db
            .list_files_for_owner(u.id)
            .await
            .unwrap_or_default();
        total_files += files.len() as i64;
        for file in &files {
            total_storage_bytes += file.size_bytes as i64;
            by_type.add(&file.content_type);
        }
    }

    Json(InstanceStats {
        total_users: users.len() as i64,
        total_files,
        total_storage_bytes,
        by_type,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct CreateApiTokenRequest {
    name: String,
}

/// Mints a new API token for the authenticated user, e.g. to paste into a
/// ShareX custom uploader config. The plaintext token is only ever returned
/// here, once.
pub async fn create_api_token(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<CreateApiTokenRequest>,
) -> Response {
    let token = core_auth::generate_token();
    let api_token = ApiToken {
        id: ApiTokenId::new(),
        token_hash: core_auth::hash_token(&token),
        user_id: user.id,
        name: body.name,
        created_at: OffsetDateTime::now_utc(),
        last_used_at: None,
    };

    if let Err(err) = state.db.create_api_token(&api_token).await {
        tracing::error!(%err, "failed to create api token");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(serde_json::json!({ "id": api_token.id.0, "token": token })).into_response()
}

#[derive(Serialize)]
pub struct ApiTokenSummary {
    id: Uuid,
    name: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    last_used_at: Option<OffsetDateTime>,
}

impl From<ApiToken> for ApiTokenSummary {
    fn from(token: ApiToken) -> Self {
        ApiTokenSummary {
            id: token.id.0,
            name: token.name,
            created_at: token.created_at,
            last_used_at: token.last_used_at,
        }
    }
}

pub async fn list_api_tokens(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    let tokens = match state.db.list_api_tokens_for_user(user.id).await {
        Ok(tokens) => tokens,
        Err(err) => {
            tracing::error!(%err, "failed to list api tokens");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let summaries: Vec<ApiTokenSummary> = tokens.into_iter().map(ApiTokenSummary::from).collect();
    Json(summaries).into_response()
}

pub async fn delete_api_token(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    let token = match state.db.get_api_token(ApiTokenId(id)).await {
        Ok(Some(token)) => token,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up api token");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if token.user_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(err) = state.db.delete_api_token(token.id).await {
        tracing::error!(%err, "failed to delete api token");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn get_embed_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    match state.db.get_embed_settings(user.id).await {
        Ok(settings) => Json(settings).into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to load embed settings");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn set_embed_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<EmbedSettings>,
) -> Response {
    if let Err(err) = state.db.set_embed_settings(user.id, &body).await {
        tracing::error!(%err, "failed to save embed settings");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Read-only: which storage backend is currently configured, for display in
/// the settings page. Never exposes credentials. Actually changing backends
/// still means editing environment variables and restarting — that config
/// isn't stored in the database.
pub async fn storage_info(AuthedUser(_): AuthedUser) -> Response {
    let backend = std::env::var("STORAGE_BACKEND").unwrap_or_default();
    if backend == "s3" {
        Json(serde_json::json!({
            "backend": "s3",
            "bucket": std::env::var("S3_BUCKET").ok(),
            "endpoint": std::env::var("S3_ENDPOINT").ok(),
        }))
        .into_response()
    } else {
        Json(serde_json::json!({
            "backend": "local",
            "root": std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./data".into()),
        }))
        .into_response()
    }
}
