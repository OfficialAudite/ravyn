use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use ravyn_core::{ChunkedUpload, ChunkedUploadId, File, FileId, UserId};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    files::{generate_thumbnail, notify_upload_webhook, MAX_BUFFERED_IMAGE_BYTES},
    resolve_public_base_url,
};
use crate::{auth::AuthedUser, state::AppState};
use ravyn_db::ChunkedUploadPart;

/// Chunk size this server hands back from `init_chunked_upload` for the
/// browser to slice by. `ravyn-web` has its own copy of the size threshold
/// that decides whether to use this path at all (`ravyn-web` never depends
/// on `ravyn-api` directly, so nothing here is actually shared code, just
/// two sides agreeing on the same numbers).
pub const CHUNK_SIZE: u32 = 16 * 1024 * 1024;

/// How long an incomplete chunked upload's parts stick around before the
/// sweep (`run_chunked_upload_sweep`) cleans them up - long enough to
/// resume after closing a laptop overnight, short enough that an
/// abandoned upload doesn't sit in storage forever.
const ABANDONED_UPLOAD_MAX_AGE: time::Duration = time::Duration::hours(24);

fn expected_part_len(total_size: i64, part_number: i32) -> Option<i64> {
    let chunk = CHUNK_SIZE as i64;
    let start = (part_number as i64 - 1).checked_mul(chunk)?;
    if part_number < 1 || start >= total_size {
        return None;
    }
    Some(chunk.min(total_size - start))
}

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

    let settings = state.db.get_upload_settings().await.unwrap_or_default();
    let original_name = settings
        .naming_scheme
        .generate(&body.original_name, settings.random_name_length);

    let expires_at = settings
        .default_expiry
        .to_duration()
        .map(|duration| OffsetDateTime::now_utc() + duration);

    let storage_key = format!("{}/{}", user.id.0, Uuid::new_v4());
    let multipart_id = match state.storage.create_parts(&storage_key).await {
        Ok(multipart_id) => multipart_id,
        Err(err) => {
            tracing::error!(%err, "failed to start multipart upload");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let upload = ChunkedUpload {
        id: ChunkedUploadId::new(),
        owner_id: user.id,
        original_name,
        content_type: ravyn_core::normalize_content_type(&body.content_type),
        total_size: body.total_size,
        created_at: OffsetDateTime::now_utc(),
        expires_at,
        storage_key: multipart_id.is_some().then_some(storage_key),
        multipart_id,
    };

    if let Err(err) = state.db.create_chunked_upload(&upload).await {
        tracing::error!(%err, "failed to create chunked upload");
        discard_upload_storage(&state, &upload).await;
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

    // Every part's size is fixed by the declared total, so nothing beyond
    // what the quota check in `init_chunked_upload` allowed can be stored.
    let Some(expected_len) = expected_part_len(upload.total_size, part_number) else {
        return (StatusCode::BAD_REQUEST, "part number out of range").into_response();
    };
    if body.len() as i64 != expected_len {
        return (
            StatusCode::BAD_REQUEST,
            format!("part {part_number} must be exactly {expected_len} bytes"),
        )
            .into_response();
    }

    let size = body.len() as i64;
    let stored = match (&upload.storage_key, &upload.multipart_id) {
        (Some(key), Some(multipart_id)) => state
            .storage
            .put_part(key, multipart_id, part_number, body)
            .await
            .map(|part_id| (None, Some(part_id))),
        _ => {
            let key = chunk_storage_key(user.id.0, id, part_number);
            state
                .storage
                .put(&key, body)
                .await
                .map(|()| (Some(key), None))
        }
    };
    let (storage_key, part_id) = match stored {
        Ok(stored) => stored,
        Err(err) => {
            tracing::error!(%err, "failed to store upload chunk");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Err(err) = state
        .db
        .upsert_chunked_upload_part(
            upload.id,
            part_number,
            storage_key.as_deref(),
            part_id.as_deref(),
            size,
        )
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

/// Turns the received parts into the final stored file. On S3 the parts
/// already are that file, so this is one call and the hash and thumbnail
/// follow in the background (`hash_in_background`); that also means no
/// duplicate notice for these, since the hash isn't known yet. On local
/// disk the chunk objects get stitched together here instead.
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

    let storage_error = || {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage error".to_string(),
        )
    };

    let (storage_key, sha256) = match (&upload.storage_key, &upload.multipart_id) {
        (Some(key), Some(multipart_id)) => {
            let part_ids = parts
                .iter()
                .map(|part| part.part_id.clone().unwrap_or_default())
                .collect();
            if let Err(err) = state
                .storage
                .complete_parts(key, multipart_id, part_ids)
                .await
            {
                tracing::error!(%err, "failed to complete multipart upload");
                return storage_error().into_response();
            }
            (key.clone(), String::new())
        }
        _ => match stitch_chunks(&state, user.id, &parts).await {
            Ok(stitched) => stitched,
            Err(()) => return storage_error().into_response(),
        },
    };

    let file = File {
        id: FileId::new(),
        owner_id: user.id,
        original_name: upload.original_name.clone(),
        storage_key,
        content_type: upload.content_type.clone(),
        size_bytes: upload.total_size as u64,
        sha256,
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

    if file.sha256.is_empty() {
        tokio::spawn(hash_in_background(state.clone(), file.clone()));
    } else if is_thumbnailable(&file) {
        // Thumbnailing needs the whole decoded image, which the stitching
        // above deliberately never held onto at once - a single extra full
        // read here, same tradeoff `save_buffered_part` already makes for
        // every image, just paid once instead of during each chunk.
        if let Ok(bytes) = state.storage.get(&file.storage_key).await {
            generate_thumbnail(&state, file.id, bytes).await;
        }
    }

    for key in parts.iter().filter_map(|part| part.storage_key.as_deref()) {
        let _ = state.storage.delete(key).await;
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

    discard_upload_storage(&state, &upload).await;

    if let Err(err) = state.db.delete_chunked_upload(upload.id).await {
        tracing::error!(%err, "failed to delete chunked upload");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Reads the stitched chunks back in order into a fresh object, hashing
/// along the way. Only for backends without `Storage::put_part`.
async fn stitch_chunks(
    state: &AppState,
    owner_id: UserId,
    parts: &[ChunkedUploadPart],
) -> Result<(String, String), ()> {
    let storage_key = format!("{}/{}", owner_id.0, Uuid::new_v4());
    let mut writer = state
        .storage
        .start_upload(&storage_key)
        .await
        .map_err(|err| {
            tracing::error!(%err, "failed to start final upload");
        })?;

    let mut hasher = Sha256::new();
    for part in parts {
        let Some(key) = &part.storage_key else {
            tracing::error!("chunk has no storage key to stitch from");
            return Err(());
        };
        let bytes = state.storage.get(key).await.map_err(|err| {
            tracing::error!(%err, "failed to read upload chunk");
        })?;
        hasher.update(&bytes);
        writer.write_chunk(bytes).await.map_err(|err| {
            tracing::error!(%err, "failed to write final upload chunk");
        })?;
    }

    writer.finish().await.map_err(|err| {
        tracing::error!(%err, "failed to finish final upload");
    })?;

    Ok((storage_key, hex::encode(hasher.finalize())))
}

fn is_thumbnailable(file: &File) -> bool {
    file.content_type.starts_with("image/") && file.size_bytes <= MAX_BUFFERED_IMAGE_BYTES as u64
}

/// Reads a finished upload once to fill in its hash, and its thumbnail
/// from the same read if it's an image small enough to have one.
async fn hash_in_background(state: AppState, file: File) {
    let mut hasher = Sha256::new();

    if is_thumbnailable(&file) {
        let bytes = match state.storage.get(&file.storage_key).await {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::error!(%err, file_id = %file.id.0, "failed to read file for hashing");
                return;
            }
        };
        hasher.update(&bytes);
        generate_thumbnail(&state, file.id, bytes).await;
    } else {
        let mut object = match state.storage.get_stream(&file.storage_key, None).await {
            Ok(object) => object,
            Err(err) => {
                tracing::error!(%err, file_id = %file.id.0, "failed to read file for hashing");
                return;
            }
        };
        while let Some(chunk) = object.body.next().await {
            match chunk {
                Ok(bytes) => hasher.update(&bytes),
                Err(err) => {
                    tracing::error!(%err, file_id = %file.id.0, "failed to read file for hashing");
                    return;
                }
            }
        }
    }

    let sha256 = hex::encode(hasher.finalize());
    if let Err(err) = state.db.set_file_sha256(file.id, &sha256).await {
        tracing::error!(%err, "failed to record file hash");
    }
}

/// Throws away whatever an unfinished upload already stored.
pub(super) async fn discard_upload_storage(state: &AppState, upload: &ChunkedUpload) {
    if let (Some(key), Some(multipart_id)) = (&upload.storage_key, &upload.multipart_id) {
        if let Err(err) = state.storage.abort_parts(key, multipart_id).await {
            tracing::warn!(%err, "failed to abort multipart upload");
        }
        return;
    }

    if let Ok(parts) = state.db.list_chunked_upload_parts(upload.id).await {
        for key in parts.iter().filter_map(|part| part.storage_key.as_deref()) {
            let _ = state.storage.delete(key).await;
        }
    }
}

/// Runs forever as a background task, same shape as
/// `files::run_expiry_sweep`: deletes any chunked upload (and its
/// already-received chunks) old enough to count as abandoned rather than
/// merely slow, and finishes hashing any file whose background hash
/// never got to run to the end.
pub async fn run_chunked_upload_sweep(state: AppState) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1800));
    loop {
        ticker.tick().await;

        for file in state.db.list_unhashed_files().await.unwrap_or_default() {
            hash_in_background(state.clone(), file).await;
        }

        let cutoff = OffsetDateTime::now_utc() - ABANDONED_UPLOAD_MAX_AGE;
        let abandoned = match state.db.list_abandoned_chunked_uploads(cutoff).await {
            Ok(uploads) => uploads,
            Err(err) => {
                tracing::error!(%err, "failed to list abandoned chunked uploads");
                continue;
            }
        };

        for upload in abandoned {
            discard_upload_storage(&state, &upload).await;
            match state.db.delete_chunked_upload(upload.id).await {
                Ok(()) => {
                    tracing::info!(upload_id = %upload.id.0, "deleted abandoned chunked upload")
                }
                Err(err) => tracing::error!(%err, "failed to delete abandoned chunked upload"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{expected_part_len, CHUNK_SIZE};

    #[test]
    fn part_lengths() {
        let chunk = CHUNK_SIZE as i64;
        let total = chunk * 2 + 10;
        assert_eq!(expected_part_len(total, 1), Some(chunk));
        assert_eq!(expected_part_len(total, 3), Some(10));
        assert_eq!(expected_part_len(total, 4), None);
        assert_eq!(expected_part_len(total, 0), None);
        assert_eq!(expected_part_len(total, -5), None);
        assert_eq!(expected_part_len(chunk, 1), Some(chunk));
        assert_eq!(expected_part_len(chunk, 2), None);
    }
}
