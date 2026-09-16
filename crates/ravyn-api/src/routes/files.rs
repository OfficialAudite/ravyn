use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use ravyn_core::{File, FileId, FolderId};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{hash_optional_password, is_authorized, password_prompt_html, FileSummary};
use crate::{auth::AuthedUser, state::AppState};

#[derive(Deserialize)]
pub struct AccessQuery {
    password: Option<String>,
}

fn thumbnail_key(id: FileId) -> String {
    format!("{}.avif", id.0)
}

pub async fn get_file(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<AccessQuery>,
    requester: Option<AuthedUser>,
) -> Response {
    let file = match state.db.get_file(FileId(id)).await {
        Ok(Some(file)) => file,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let requester = requester.map(|AuthedUser(user)| user);
    if !is_authorized(&file, requester.as_ref(), query.password.as_deref()) {
        let message = if query.password.is_some() {
            "wrong password"
        } else {
            "this file is password protected"
        };
        return (
            StatusCode::UNAUTHORIZED,
            Html(password_prompt_html(message)),
        )
            .into_response();
    }

    let bytes = match state.storage.get(&file.storage_key).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(%err, "failed to read file from storage");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    ([(header::CONTENT_TYPE, file.content_type)], bytes).into_response()
}

/// A small local-disk preview, generated at upload time for images. Kept
/// separate from `get_file` so the gallery grid never has to pull a
/// multi-megabyte original (possibly from S3) just to render a thumbnail.
pub async fn get_file_thumbnail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<AccessQuery>,
    requester: Option<AuthedUser>,
) -> Response {
    let file = match state.db.get_file(FileId(id)).await {
        Ok(Some(file)) => file,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let requester = requester.map(|AuthedUser(user)| user);
    if !is_authorized(&file, requester.as_ref(), query.password.as_deref()) {
        return StatusCode::FORBIDDEN.into_response();
    }

    match state.thumbnails.get(&thumbnail_key(file.id)).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "image/avif")], bytes).into_response(),
        Err(err) if err.is_not_found() => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to read thumbnail");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Best-effort: a thumbnail is a nice-to-have, not something an upload
/// should fail over. Anything that isn't a decodable still image (a corrupt
/// file, a format we don't handle) is silently skipped. Awaited before the
/// upload response returns — resizing to 400x400 first keeps the AVIF
/// encode itself cheap regardless of the original's size, and awaiting it
/// guarantees a file never shows up in a list before its thumbnail exists
/// (a backgrounded version of this raced the dashboard's first render).
async fn generate_thumbnail(state: &AppState, id: FileId, bytes: &[u8]) {
    let Ok(image) = image::load_from_memory(bytes) else {
        return;
    };

    let mut avif = Vec::new();
    let encoder = image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut avif, 6, 70);
    if image
        .thumbnail(400, 400)
        .write_with_encoder(encoder)
        .is_err()
    {
        return;
    }

    if let Err(err) = state.thumbnails.put(&thumbnail_key(id), avif.into()).await {
        tracing::warn!(%err, "failed to store thumbnail");
    }
}

/// Accepts a single-part multipart upload. This is what a ShareX custom
/// uploader config points at, with the API token in the `Authorization`
/// header.
pub async fn upload_file(
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
        content_type: content_type.clone(),
        size_bytes: bytes.len() as u64,
        sha256,
        folder_id: None,
        password_hash: None,
        created_at: OffsetDateTime::now_utc(),
    };

    if let Err(err) = state.db.insert_file(&file).await {
        tracing::error!(%err, "failed to record uploaded file");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if content_type.starts_with("image/") {
        generate_thumbnail(&state, file.id, &bytes).await;
    }

    Json(serde_json::json!({ "id": file.id.0 })).into_response()
}

pub async fn list_files(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
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

pub async fn delete_file(
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

    let _ = state.thumbnails.delete(&thumbnail_key(file.id)).await;

    if let Err(err) = state.db.delete_file(file.id).await {
        tracing::error!(%err, "failed to delete file record");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub struct SetFileFolderRequest {
    folder_id: Option<Uuid>,
}

pub async fn set_file_folder(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetFileFolderRequest>,
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

    if let Some(folder_id) = body.folder_id {
        match state.db.get_folder(FolderId(folder_id)).await {
            Ok(Some(folder)) if folder.owner_id == user.id => {}
            Ok(Some(_)) => return StatusCode::FORBIDDEN.into_response(),
            Ok(None) => return StatusCode::NOT_FOUND.into_response(),
            Err(err) => {
                tracing::error!(%err, "failed to look up folder");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }

    if let Err(err) = state
        .db
        .set_file_folder(file.id, body.folder_id.map(FolderId))
        .await
    {
        tracing::error!(%err, "failed to move file");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub struct SetPasswordRequest {
    password: Option<String>,
}

pub async fn set_file_password(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetPasswordRequest>,
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

    let hash = match hash_optional_password(body.password) {
        Ok(hash) => hash,
        Err(status) => return status.into_response(),
    };

    if let Err(err) = state.db.set_file_password(file.id, hash).await {
        tracing::error!(%err, "failed to set file password");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}
