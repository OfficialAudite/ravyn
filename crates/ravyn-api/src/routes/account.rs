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
    auth as core_auth, ApiToken, ApiTokenId, EmbedSettings, Invite, InviteId, RegistrationMode,
    User, UserId,
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
pub struct SetInstanceSettingsRequest {
    registration_mode: String,
}

pub async fn get_instance_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    match state.db.get_registration_mode().await {
        Ok(mode) => Json(serde_json::json!({ "registration_mode": mode.as_str() })).into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to load registration mode");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn set_instance_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<SetInstanceSettingsRequest>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let Some(mode) = RegistrationMode::parse(&body.registration_mode) else {
        return (StatusCode::BAD_REQUEST, "invalid registration mode").into_response();
    };

    if let Err(err) = state.db.set_registration_mode(mode).await {
        tracing::error!(%err, "failed to save registration mode");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
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
        summaries.push(AdminUserSummary {
            id: u.id.0,
            username: u.username,
            is_admin: u.is_admin,
            created_at: u.created_at,
            storage_used_bytes,
            max_storage_bytes,
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
