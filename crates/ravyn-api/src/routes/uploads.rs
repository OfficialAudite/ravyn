use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use ravyn_core::{ChunkedUpload, ChunkedUploadId, File, FileId};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{files::notify_upload_webhook, resolve_public_base_url};
use crate::{auth::AuthedUser, state::AppState};

/// Chunk size this server hands back from `init_chunked_upload` for the
/// browser to slice by. `ravyn-web` has its own copy of the size threshold
/// that decides whether to use this path at all (`ravyn-web` never depends
/// on `ravyn-api` directly, so nothing here is actually shared code, just
/// two sides agreeing on the same numbers).
pub const CHUNK_SIZE: u32 = 8 * 1024 * 1024;

/// How long an incomplete chunked upload's parts stick around before the
/// sweep (`run_chunked_upload_sweep`) cleans them up - long enough to
/// resume after closing a laptop overnight, short enough that an
/// abandoned upload doesn't sit in storage forever.
const ABANDONED_UPLOAD_MAX_AGE: time::Duration = time::Duration::hours(24);

fn chunk_storage_key(owner_id: Uuid, upload_id: Uuid, part_number: i32) -> String {
    format!("chunks/{owner_id}/{upload_id}/{part_number}")
}

#[derive(Deserialize)]
pub struct InitChunkedUploadRequest {
    original_name: String,
    content_type: String,
    total_size: i64,
}

/// Starts a chunked upload: checks the quota against the declared total
/// size up front (the chunked path knows this ahead of time, unlike the
/// streamed single-request path, which only learns a part's size as it
/// arrives), applies the instance's naming scheme and default expiry the
/// same way a regular upload does, and hands back an id the browser sends
/// each chunk against.
pub async fn init_chunked_upload(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<InitChunkedUploadRequest>,
) -> Response {
    if body.total_size <= 0 {
        return (StatusCode::BAD_REQUEST, "total_size must be positive").into_response();
    }

    match state.db.get_max_storage_bytes(user.id).await {
        Ok(Some(max)) => {
            let used = state.db.get_storage_usage(user.id).await.unwrap_or(0);
            if used + body.total_size > max {
                return (StatusCode::INSUFFICIENT_STORAGE, "storage limit exceeded")
                    .into_response();
            }
        }
        Ok(None) => {}
        Err(err) => {
            tracing::error!(%err, "failed to check storage quota");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    let naming_scheme = state
        .db
        .get_naming_scheme()
        .await
        .unwrap_or(ravyn_core::NamingScheme::Original);
    let random_name_length = state.db.get_random_name_length().await.unwrap_or(8);
    let original_name = naming_scheme.generate(&body.original_name, random_name_length as usize);

    let expires_at = state
        .db
        .get_default_expiry_preset()
        .await
        .unwrap_or(ravyn_core::ExpiryPreset::Never)
        .to_duration()
        .map(|duration| OffsetDateTime::now_utc() + duration);

    let upload = ChunkedUpload {
        id: ChunkedUploadId::new(),
        owner_id: user.id,
        original_name,
        content_type: body.content_type,
        total_size: body.total_size,
        created_at: OffsetDateTime::now_utc(),
        expires_at,
    };

    if let Err(err) = state.db.create_chunked_upload(&upload).await {
        tracing::error!(%err, "failed to create chunked upload");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(serde_json::json!({
        "upload_id": upload.id.0,
        "chunk_size": CHUNK_SIZE,
    }))
    .into_response()
}

/// Accepts one chunk's raw bytes, no multipart wrapper - `ravyn-web`'s own
/// proxy for this route streams the request body straight through, and
/// `Bytes` here is fine specifically because a chunk is deliberately kept
/// small (`CHUNK_SIZE`), the same reasoning `save_buffered_part` already
/// relies on for whole images.
pub async fn upload_chunk(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path((id, part_number)): Path<(Uuid, i32)>,
    body: Bytes,
) -> Response {
    let upload = match state.db.get_chunked_upload(ChunkedUploadId(id)).await {
        Ok(Some(upload)) => upload,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up chunked upload");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if upload.owner_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let key = chunk_storage_key(user.id.0, id, part_number);
    if let Err(err) = state.storage.put(&key, body.clone()).await {
        tracing::error!(%err, "failed to store upload chunk");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if let Err(err) = state
        .db
        .upsert_chunked_upload_part(upload.id, part_number, &key, body.len() as i64)
        .await
    {
        tracing::error!(%err, "failed to record upload chunk");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let received_bytes: i64 = match state.db.list_chunked_upload_parts(upload.id).await {
        Ok(parts) => parts.iter().map(|part| part.size_bytes).sum(),
        Err(err) => {
            tracing::error!(%err, "failed to sum upload parts");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(serde_json::json!({ "received_bytes": received_bytes })).into_response()
}

/// Lets the browser figure out where to resume after a reload: which part
/// numbers already made it to the server, and how many bytes that adds up
/// to against the declared total.
pub async fn get_chunked_upload_status(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    let upload = match state.db.get_chunked_upload(ChunkedUploadId(id)).await {
        Ok(Some(upload)) => upload,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up chunked upload");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if upload.owner_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let parts = match state.db.list_chunked_upload_parts(upload.id).await {
        Ok(parts) => parts,
        Err(err) => {
            tracing::error!(%err, "failed to list upload parts");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let received_bytes: i64 = parts.iter().map(|part| part.size_bytes).sum();
    let part_numbers: Vec<i32> = parts.iter().map(|part| part.part_number).collect();

    Json(serde_json::json!({
        "received_bytes": received_bytes,
        "total_size": upload.total_size,
        "parts": part_numbers,
    }))
    .into_response()
}

/// Reassembles every part, in order, into the final stored file - reads
/// each temporary chunk object back and rewrites it into a fresh
/// multipart upload, the same `Storage::start_upload`/`write_chunk`
/// primitives `save_streamed_part` already uses for a single-request
/// upload. Costs an extra read per chunk over a true server-side
/// concatenation, but works identically on local disk and S3 without
/// either backend needing anything beyond `get`/`put`/`delete`.
pub async fn complete_chunked_upload(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let upload = match state.db.get_chunked_upload(ChunkedUploadId(id)).await {
        Ok(Some(upload)) => upload,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up chunked upload");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if upload.owner_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let parts = match state.db.list_chunked_upload_parts(upload.id).await {
        Ok(parts) => parts,
        Err(err) => {
            tracing::error!(%err, "failed to list upload parts");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let received_bytes: i64 = parts.iter().map(|part| part.size_bytes).sum();
    if received_bytes != upload.total_size {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "upload incomplete: received {received_bytes} of {} bytes",
                upload.total_size
            ),
        )
            .into_response();
    }
    for (index, part) in parts.iter().enumerate() {
        if part.part_number != index as i32 + 1 {
            return (StatusCode::BAD_REQUEST, "missing a chunk in the sequence").into_response();
        }
    }

    let storage_key = format!("{}/{}", user.id.0, Uuid::new_v4());
    let storage_error = || {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage error".to_string(),
        )
    };

    let mut writer = match state.storage.start_upload(&storage_key).await {
        Ok(writer) => writer,
        Err(err) => {
            tracing::error!(%err, "failed to start final upload");
            return storage_error().into_response();
        }
    };

    let mut hasher = Sha256::new();
    for part in &parts {
        let bytes = match state.storage.get(&part.storage_key).await {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::error!(%err, "failed to read upload chunk");
                return storage_error().into_response();
            }
        };
        hasher.update(&bytes);
        if let Err(err) = writer.write_chunk(bytes).await {
            tracing::error!(%err, "failed to write final upload chunk");
            return storage_error().into_response();
        }
    }

    if let Err(err) = writer.finish().await {
        tracing::error!(%err, "failed to finish final upload");
        return storage_error().into_response();
    }

    let file = File {
        id: FileId::new(),
        owner_id: user.id,
        original_name: upload.original_name.clone(),
        storage_key,
        content_type: upload.content_type.clone(),
        size_bytes: upload.total_size as u64,
        sha256: hex::encode(hasher.finalize()),
        folder_id: None,
        password_hash: None,
        created_at: OffsetDateTime::now_utc(),
        expires_at: upload.expires_at,
        tags: Vec::new(),
    };

    if let Err(err) = state.db.insert_file(&file).await {
        tracing::error!(%err, "failed to record uploaded file");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "database error".to_string(),
        )
            .into_response();
    }

    // Thumbnailing needs the whole decoded image, which the streaming
    // reassembly above deliberately never held onto at once - a single
    // extra full read here, same tradeoff `save_buffered_part` already
    // makes for every image, just paid once instead of during each chunk.
    if file.content_type.starts_with("image/") {
        if let Ok(bytes) = state.storage.get(&file.storage_key).await {
            super::files::generate_thumbnail(&state, file.id, &bytes).await;
        }
    }

    for part in &parts {
        let _ = state.storage.delete(&part.storage_key).await;
    }
    if let Err(err) = state.db.delete_chunked_upload(upload.id).await {
        tracing::warn!(%err, "failed to clean up chunked upload record");
    }

    let base_url = resolve_public_base_url(&state, &headers);
    notify_upload_webhook(&state, user.id, &file, &base_url).await;

    let duplicate_of = state
        .db
        .find_duplicate_for_owner(user.id, &file.sha256, file.id)
        .await
        .unwrap_or_default()
        .map(|existing| existing.id.0);

    Json(serde_json::json!({ "id": file.id.0, "duplicate_of": duplicate_of })).into_response()
}

pub async fn cancel_chunked_upload(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    let upload = match state.db.get_chunked_upload(ChunkedUploadId(id)).await {
        Ok(Some(upload)) => upload,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up chunked upload");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if upload.owner_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Ok(parts) = state.db.list_chunked_upload_parts(upload.id).await {
        for part in parts {
            let _ = state.storage.delete(&part.storage_key).await;
        }
    }

    if let Err(err) = state.db.delete_chunked_upload(upload.id).await {
        tracing::error!(%err, "failed to delete chunked upload");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Runs forever as a background task, same shape as
/// `files::run_expiry_sweep`: deletes any chunked upload (and its
/// already-received chunks) old enough to count as abandoned rather than
/// merely slow.
pub async fn run_chunked_upload_sweep(state: AppState) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1800));
    loop {
        ticker.tick().await;

        let cutoff = OffsetDateTime::now_utc() - ABANDONED_UPLOAD_MAX_AGE;
        let abandoned = match state.db.list_abandoned_chunked_uploads(cutoff).await {
            Ok(uploads) => uploads,
            Err(err) => {
                tracing::error!(%err, "failed to list abandoned chunked uploads");
                continue;
            }
        };

        for upload in abandoned {
            if let Ok(parts) = state.db.list_chunked_upload_parts(upload.id).await {
                for part in parts {
                    let _ = state.storage.delete(&part.storage_key).await;
                }
            }
            match state.db.delete_chunked_upload(upload.id).await {
                Ok(()) => {
                    tracing::info!(upload_id = %upload.id.0, "deleted abandoned chunked upload")
                }
                Err(err) => tracing::error!(%err, "failed to delete abandoned chunked upload"),
            }
        }
    }
}
