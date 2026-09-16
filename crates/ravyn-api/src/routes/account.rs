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
use ravyn_core::{auth as core_auth, ApiToken, ApiTokenId};
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

    let token = core_auth::generate_token();
    let session = ravyn_core::Session {
        token_hash: core_auth::hash_token(&token),
        user_id: user.id,
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

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        let token_hash = core_auth::hash_token(cookie.value());
        let _ = state.db.delete_session(&token_hash).await;
    }

    (jar.remove(Cookie::from(SESSION_COOKIE)), StatusCode::OK).into_response()
}

pub async fn me(AuthedUser(user): AuthedUser) -> Response {
    Json(serde_json::json!({ "username": user.username })).into_response()
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
