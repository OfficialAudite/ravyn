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

/// One field's worth of a multipart upload, saved. Pulled out of
/// `upload_file` so it can be called once per part — the web UI's dropzone
/// sends one request with several parts when you drop multiple files at
/// once, ShareX always sends exactly one.
///
/// Images go through `save_buffered_part`: thumbnailing needs the whole
/// image decoded in memory regardless, and images are small enough that
/// buffering them costs nothing extra. Everything else — video in
/// particular, which is exactly what people hit multi-gigabyte uploads
/// with — goes through `save_streamed_part`, which never holds more than
/// one chunk of the file in memory at a time.
async fn save_uploaded_part(
    state: &AppState,
    owner_id: ravyn_core::UserId,
    field: axum::extract::multipart::Field<'_>,
) -> Result<File, (StatusCode, String)> {
    let uploaded_name = field.file_name().unwrap_or("upload").to_string();
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();

    // Instance-wide, admin-configurable: what a freshly uploaded file gets
    // called by default. Falls back to keeping the uploader's own filename
    // on any lookup error, same as every other settings read on this path.
    let naming_scheme = state
        .db
        .get_naming_scheme()
        .await
        .unwrap_or(ravyn_core::NamingScheme::Original);
    let random_name_length = state.db.get_random_name_length().await.unwrap_or(8);
    let original_name = naming_scheme.generate(&uploaded_name, random_name_length as usize);

    if content_type.starts_with("image/") {
        save_buffered_part(state, owner_id, field, original_name, content_type).await
    } else {
        save_streamed_part(state, owner_id, field, original_name, content_type).await
    }
}

async fn save_buffered_part(
    state: &AppState,
    owner_id: ravyn_core::UserId,
    field: axum::extract::multipart::Field<'_>,
    original_name: String,
    content_type: String,
) -> Result<File, (StatusCode, String)> {
    let bytes = field
        .bytes()
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;

    match state.db.get_max_storage_bytes(owner_id).await {
        Ok(Some(max)) => {
            let used = state.db.get_storage_usage(owner_id).await.map_err(|err| {
                tracing::error!(%err, "failed to check storage usage");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "storage error".to_string(),
                )
            })?;
            if used + bytes.len() as i64 > max {
                return Err((
                    StatusCode::INSUFFICIENT_STORAGE,
                    "storage limit exceeded".to_string(),
                ));
            }
        }
        Ok(None) => {}
        Err(err) => {
            tracing::error!(%err, "failed to check storage quota");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage error".to_string(),
            ));
        }
    }

    let sha256 = hex::encode(Sha256::digest(&bytes));
    let storage_key = format!("{}/{}", owner_id.0, Uuid::new_v4());

    state
        .storage
        .put(&storage_key, bytes.clone())
        .await
        .map_err(|err| {
            tracing::error!(%err, "failed to write file to storage");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage error".to_string(),
            )
        })?;

    let file = File {
        id: FileId::new(),
        owner_id,
        original_name,
        storage_key,
        content_type,
        size_bytes: bytes.len() as u64,
        sha256,
        folder_id: None,
        password_hash: None,
        created_at: OffsetDateTime::now_utc(),
    };

    state.db.insert_file(&file).await.map_err(|err| {
        tracing::error!(%err, "failed to record uploaded file");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "database error".to_string(),
        )
    })?;

    generate_thumbnail(state, file.id, &bytes).await;

    Ok(file)
}

/// Same job as `save_buffered_part`, but a chunk at a time: the hash and
/// size are computed incrementally as chunks arrive, each chunk goes
/// straight to storage via `StorageUpload` rather than into a `Bytes`
/// buffer, and the quota is checked against the running total after every
/// chunk rather than once up front — up front isn't possible here, since
/// nothing tells us a part's total size before we've read all of it. A
/// quota violation aborts the in-progress storage upload before it's ever
/// recorded in the database, rather than writing the whole oversized file
/// and only then discovering it shouldn't have been kept.
async fn save_streamed_part(
    state: &AppState,
    owner_id: ravyn_core::UserId,
    mut field: axum::extract::multipart::Field<'_>,
    original_name: String,
    content_type: String,
) -> Result<File, (StatusCode, String)> {
    let storage_error = || {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage error".to_string(),
        )
    };

    let max_storage_bytes = state
        .db
        .get_max_storage_bytes(owner_id)
        .await
        .map_err(|err| {
            tracing::error!(%err, "failed to check storage quota");
            storage_error()
        })?;

    let mut used: i64 = 0;
    if let Some(max) = max_storage_bytes {
        used = state.db.get_storage_usage(owner_id).await.map_err(|err| {
            tracing::error!(%err, "failed to check storage usage");
            storage_error()
        })?;
        if used >= max {
            return Err((
                StatusCode::INSUFFICIENT_STORAGE,
                "storage limit exceeded".to_string(),
            ));
        }
    }

    let storage_key = format!("{}/{}", owner_id.0, Uuid::new_v4());
    let mut upload = state
        .storage
        .start_upload(&storage_key)
        .await
        .map_err(|err| {
            tracing::error!(%err, "failed to start upload");
            storage_error()
        })?;

    let mut hasher = Sha256::new();
    let mut size_bytes: u64 = 0;

    loop {
        let chunk = match field.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(err) => {
                let _ = upload.abort().await;
                return Err((StatusCode::BAD_REQUEST, err.to_string()));
            }
        };

        size_bytes += chunk.len() as u64;
        if let Some(max) = max_storage_bytes {
            if used + size_bytes as i64 > max {
                let _ = upload.abort().await;
                return Err((
                    StatusCode::INSUFFICIENT_STORAGE,
                    "storage limit exceeded".to_string(),
                ));
            }
        }

        hasher.update(&chunk);
        if let Err(err) = upload.write_chunk(chunk).await {
            tracing::error!(%err, "failed to write upload chunk");
            return Err(storage_error());
        }
    }

    upload.finish().await.map_err(|err| {
        tracing::error!(%err, "failed to finish upload");
        storage_error()
    })?;

    let file = File {
        id: FileId::new(),
        owner_id,
        original_name,
        storage_key,
        content_type,
        size_bytes,
        sha256: hex::encode(hasher.finalize()),
        folder_id: None,
        password_hash: None,
        created_at: OffsetDateTime::now_utc(),
    };

    state.db.insert_file(&file).await.map_err(|err| {
        tracing::error!(%err, "failed to record uploaded file");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "database error".to_string(),
        )
    })?;

    Ok(file)
}

struct UploadOutcome {
    name: String,
    result: Result<Uuid, (StatusCode, String)>,
}

#[derive(serde::Serialize)]
struct UploadOutcomeJson {
    name: String,
    id: Option<Uuid>,
    error: Option<String>,
}

impl From<&UploadOutcome> for UploadOutcomeJson {
    fn from(outcome: &UploadOutcome) -> Self {
        match &outcome.result {
            Ok(id) => UploadOutcomeJson {
                name: outcome.name.clone(),
                id: Some(*id),
                error: None,
            },
            Err((_, message)) => UploadOutcomeJson {
                name: outcome.name.clone(),
                id: None,
                error: Some(message.clone()),
            },
        }
    }
}

/// Accepts one or more parts in a single multipart request — the dropzone
/// sends several when you drop multiple files at once. A single-part
/// request (what ShareX always sends) keeps the old `{"id": ...}` response
/// shape (with the real status code on failure — a quota rejection is
/// still `507`, not a generic `500`) so existing ShareX configs
/// (`{json:id}`) keep working unchanged; with more than one part the
/// response is a `[{"name","id","error"}]` array instead, since there's no
/// longer one single id or status to report and a later file hitting its
/// owner's quota shouldn't lose the ones already saved before it.
pub async fn upload_file(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Response {
    let mut outcomes = Vec::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(%err, "invalid multipart body");
                if outcomes.is_empty() {
                    return StatusCode::BAD_REQUEST.into_response();
                }
                break;
            }
        };

        let name = field.file_name().unwrap_or("upload").to_string();
        let result = save_uploaded_part(&state, user.id, field)
            .await
            .map(|file| file.id.0);
        outcomes.push(UploadOutcome { name, result });
    }

    if outcomes.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    if let [only] = outcomes.as_slice() {
        return match &only.result {
            Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
            Err((status, message)) => (*status, message.clone()).into_response(),
        };
    }

    let json: Vec<UploadOutcomeJson> = outcomes.iter().map(UploadOutcomeJson::from).collect();
    Json(json).into_response()
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

#[derive(Deserialize)]
pub struct SetFileNameRequest {
    name: String,
}

/// Manual per-upload naming control: whatever the instance's naming scheme
/// assigned at upload time is just the default, not the last word — the
/// owner can always override it here.
pub async fn set_file_name(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetFileNameRequest>,
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

    let name = body.name.trim().to_string();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name cannot be empty").into_response();
    }

    if let Err(err) = state.db.set_file_name(file.id, name).await {
        tracing::error!(%err, "failed to rename file");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}
