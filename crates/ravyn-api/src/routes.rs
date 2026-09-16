use axum::{
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use ravyn_core::{auth as core_auth, ApiToken, File, FileId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    auth::{AuthedUser, SESSION_COOKIE, SESSION_LIFETIME},
    state::AppState,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/files/{id}", get(get_file))
        .route("/files/{id}", delete(delete_file))
        .route("/files", get(list_files))
        .route("/files", post(upload_file))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/api-tokens", post(create_api_token))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn get_file(State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    let file = match state.db.get_file(FileId(id)).await {
        Ok(Some(file)) => file,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let bytes = match state.storage.get(&file.storage_key).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(%err, "failed to read file from storage");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    ([(header::CONTENT_TYPE, file.content_type)], bytes).into_response()
}

/// Accepts a single-part multipart upload. This is what a ShareX custom
/// uploader config points at, with the API token in the `Authorization`
/// header.
async fn upload_file(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Response {
    let field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => return StatusCode::BAD_REQUEST.into_response(),
        Err(err) => {
            tracing::warn!(%err, "invalid multipart body");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    let original_name = field.file_name().unwrap_or("upload").to_string();
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();

    let bytes = match field.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!(%err, "failed to read upload body");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    let sha256 = hex::encode(Sha256::digest(&bytes));
    let storage_key = format!("{}/{}", user.id.0, Uuid::new_v4());

    if let Err(err) = state.storage.put(&storage_key, bytes.clone()).await {
        tracing::error!(%err, "failed to write file to storage");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let file = File {
        id: FileId::new(),
        owner_id: user.id,
        original_name,
        storage_key,
        content_type,
        size_bytes: bytes.len() as u64,
        sha256,
        created_at: OffsetDateTime::now_utc(),
    };

    if let Err(err) = state.db.insert_file(&file).await {
        tracing::error!(%err, "failed to record uploaded file");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(serde_json::json!({ "id": file.id.0 })).into_response()
}

#[derive(Serialize)]
struct FileSummary {
    id: Uuid,
    original_name: String,
    content_type: String,
    size_bytes: u64,
    sha256: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

impl From<File> for FileSummary {
    fn from(file: File) -> Self {
        FileSummary {
            id: file.id.0,
            original_name: file.original_name,
            content_type: file.content_type,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
            created_at: file.created_at,
        }
    }
}

async fn list_files(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    let files = match state.db.list_files_for_owner(user.id).await {
        Ok(files) => files,
        Err(err) => {
            tracing::error!(%err, "failed to list files");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let summaries: Vec<FileSummary> = files.into_iter().map(FileSummary::from).collect();
    Json(summaries).into_response()
}

async fn delete_file(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    let file = match state.db.get_file(FileId(id)).await {
        Ok(Some(file)) => file,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if file.owner_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(err) = state.storage.delete(&file.storage_key).await {
        tracing::error!(%err, "failed to delete file from storage");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if let Err(err) = state.db.delete_file(file.id).await {
        tracing::error!(%err, "failed to delete file record");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

async fn login(State(state): State<AppState>, Json(body): Json<LoginRequest>) -> Response {
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

async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        let token_hash = core_auth::hash_token(cookie.value());
        let _ = state.db.delete_session(&token_hash).await;
    }

    (jar.remove(Cookie::from(SESSION_COOKIE)), StatusCode::OK).into_response()
}

#[derive(Deserialize)]
struct CreateApiTokenRequest {
    name: String,
}

/// Mints a new API token for the authenticated user, e.g. to paste into a
/// ShareX custom uploader config. The plaintext token is only ever returned
/// here, once.
async fn create_api_token(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<CreateApiTokenRequest>,
) -> Response {
    let token = core_auth::generate_token();
    let api_token = ApiToken {
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

    Json(serde_json::json!({ "token": token })).into_response()
}
