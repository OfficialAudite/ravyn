use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use ravyn_core::{File, FileId, FolderId, UserId};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    hash_optional_password, is_authorized, password_prompt_html, resolve_public_base_url,
    FileSummary,
};
use crate::{auth::AuthedUser, state::AppState};

#[derive(Deserialize)]
pub struct AccessQuery {
    password: Option<String>,
}

pub(super) fn thumbnail_key(id: FileId) -> String {
    format!("{}.avif", id.0)
}

/// Whether serving this content type as the direct response to a
/// top-level navigation (someone opening the link itself, not an `<img>`/
/// `<video>` embedding it) would let an uploader's own markup or script
/// run as if it were part of this site. `content_type` comes straight
/// from whatever the uploader's client declared at upload time
/// (`save_uploaded_part`), so it's attacker-controlled: a file uploaded
/// with `Content-Type: text/html` and a `<script>` body would otherwise
/// execute with this origin's session cookie in scope for anyone who
/// opens it directly. An `<img>`/`<video>`/`<audio>` element never
/// executes script from what it loads regardless of type, so this only
/// needs to guard the direct-navigation case, not image previews
/// elsewhere in the app - an SVG still renders fine as an `<img>`.
fn unsafe_for_inline_navigation(content_type: &str) -> bool {
    matches!(
        content_type,
        "text/html" | "application/xhtml+xml" | "image/svg+xml" | "text/xml" | "application/xml"
    )
}

/// Adds `X-Content-Type-Options: nosniff` always, and a `Content-Disposition`
/// carrying the file's real name (extension included) - `attachment` for
/// anything `unsafe_for_inline_navigation` flags, `inline` otherwise. Either
/// way the `filename` param is what the browser saves the file as, so a type
/// it can't render inline (an `.apk`, say) still downloads under its real
/// name instead of falling back to the bare, extension-less id in the URL.
/// `inline` for the safe majority preserves today's behavior for images,
/// video, audio, PDFs, and the like - the browser previews them exactly as
/// it did with no header at all, since it only downloads what it can't
/// render inline regardless of the disposition type. Shared by every route
/// that serves a file's original bytes (`GET /files/{id}`, and `/v/{id}`'s
/// redirect ends up here too), so the decision only has to be made in one
/// place.
fn file_response_headers(content_type: &str, original_name: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());

    let disposition = if unsafe_for_inline_navigation(content_type) {
        "attachment"
    } else {
        "inline"
    };
    let safe_name = original_name.replace(['"', '\\', '\r', '\n'], "_");
    let value = format!("{disposition}; filename=\"{safe_name}\"");
    if let Ok(value) = value.parse() {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }

    headers
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

    let mut headers = file_response_headers(&file.content_type, &file.original_name);
    headers.insert(header::CONTENT_TYPE, file.content_type.parse().unwrap());
    (headers, bytes).into_response()
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
pub(super) async fn generate_thumbnail(state: &AppState, id: FileId, bytes: &[u8]) {
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

/// Strips EXIF metadata (camera model, GPS coordinates, timestamps, etc.)
/// from an uploaded image before it's stored — same reasoning as
/// `generate_thumbnail`'s best-effort approach: `img-parts` only
/// understands JPEG/PNG/WebP, so anything else (GIF, AVIF) is returned
/// untouched rather than failing the upload over a privacy nice-to-have.
/// Rewrites the container in place rather than fully decoding and
/// re-encoding the image, so this never touches pixel data or
/// recompresses — the file this returns is byte-identical to the original
/// except for the removed EXIF segment.
fn strip_exif(bytes: axum::body::Bytes) -> axum::body::Bytes {
    use img_parts::ImageEXIF;

    let Ok(Some(mut image)) = img_parts::DynImage::from_bytes(bytes.clone()) else {
        return bytes;
    };
    if image.exif().is_none() {
        return bytes;
    }

    image.set_exif(None);
    image.encoder().bytes()
}

/// Re-encodes a decoded image into `format` at `quality` (1-100), lossy -
/// this is an explicit size/quality trade-off an admin or uploader opted
/// into (see `ravyn_core::ImageCompressionFormat`'s own docs for why only
/// these two formats), not something applied silently. `None` on any
/// failure to decode or encode, left to the caller to fall back to the
/// original bytes untouched.
fn compress_image(
    bytes: &[u8],
    format: ravyn_core::ImageCompressionFormat,
    quality: u8,
) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let mut out = Vec::new();

    match format {
        ravyn_core::ImageCompressionFormat::Jpeg => {
            // JPEG has no alpha channel - dropping it here rather than
            // letting the encoder reject (or silently mangle) an RGBA
            // source is the same "flatten transparency" behavior any
            // other PNG-to-JPEG conversion has to make.
            let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
            rgb.write_with_encoder(encoder).ok()?;
        }
        ravyn_core::ImageCompressionFormat::Avif => {
            // Same encoder `generate_thumbnail` already uses - speed 6 is
            // that function's own middle-of-the-road choice, reused here
            // for the same reason: fast enough not to make an upload feel
            // like it hung, without giving up much compression for it.
            let encoder =
                image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut out, 6, quality);
            img.write_with_encoder(encoder).ok()?;
        }
    }

    Some(out)
}

/// Swaps (or adds) `name`'s extension - used when compression changes a
/// file's actual format, so a renamed-in-place "screenshot.png" that's
/// now really a JPEG doesn't keep a misleading extension.
fn replace_extension(name: &str, new_extension: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) => format!("{stem}.{new_extension}"),
        None => format!("{name}.{new_extension}"),
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
    compress_format_override: Option<Option<ravyn_core::ImageCompressionFormat>>,
    compress_quality_override: Option<u8>,
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

    // Same idea, for auto-delete: the instance's default expiry applies
    // unless the owner sets one of their own afterward (see
    // `set_file_expiry`) — falls back to no expiry, the safe direction,
    // rather than the sweep.
    let expires_at = state
        .db
        .get_default_expiry_preset()
        .await
        .unwrap_or(ravyn_core::ExpiryPreset::Never)
        .to_duration()
        .map(|duration| OffsetDateTime::now_utc() + duration);

    if content_type.starts_with("image/") {
        // An explicit per-upload override always wins, including an
        // explicit "off" (`Some(None)`, see the field-parsing comment
        // above for why that's distinct from no override at all);
        // otherwise fall back to whatever the instance's own default is
        // (which may itself be "off"). A quality on its own with no
        // format does nothing, same as the reverse: both are meaningless
        // alone.
        let compression = match compress_format_override {
            Some(Some(format)) => Some((format, compress_quality_override.unwrap_or(80))),
            Some(None) => None,
            None => {
                let (format, quality) = state
                    .db
                    .get_compression_settings()
                    .await
                    .unwrap_or((None, None));
                format.map(|format| (format, quality.map(ravyn_core::clamp_quality).unwrap_or(80)))
            }
        };

        save_buffered_part(
            state,
            owner_id,
            field,
            original_name,
            content_type,
            expires_at,
            compression,
        )
        .await
    } else {
        save_streamed_part(
            state,
            owner_id,
            field,
            original_name,
            content_type,
            expires_at,
        )
        .await
    }
}

async fn save_buffered_part(
    state: &AppState,
    owner_id: ravyn_core::UserId,
    field: axum::extract::multipart::Field<'_>,
    original_name: String,
    content_type: String,
    expires_at: Option<OffsetDateTime>,
    compression: Option<(ravyn_core::ImageCompressionFormat, u8)>,
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

    // Quota was already checked against the pre-strip size above — stripping
    // only ever removes bytes, so re-checking after wouldn't change the
    // outcome.
    let bytes = if state.db.get_strip_exif().await.unwrap_or(true) {
        strip_exif(bytes)
    } else {
        bytes
    };

    // Same quota reasoning as stripping above: a lossy re-encode only
    // ever shrinks a real photo/screenshot, so there's nothing to
    // re-check. Best-effort like every other image transform on this
    // path - a source `image` can't decode (an animated GIF, a format
    // outside what this build supports) uploads untouched rather than
    // failing over what's an optional size trade-off to begin with.
    let (bytes, content_type, original_name) = match compression {
        Some((format, quality)) => match compress_image(&bytes, format, quality) {
            Some(compressed) => (
                compressed.into(),
                format.content_type().to_string(),
                replace_extension(&original_name, format.extension()),
            ),
            None => (bytes, content_type, original_name),
        },
        None => (bytes, content_type, original_name),
    };

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
        expires_at,
        tags: Vec::new(),
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
    expires_at: Option<OffsetDateTime>,
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
        expires_at,
        tags: Vec::new(),
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

/// Posts a Discord/Slack-compatible notification for a freshly uploaded
/// file, if its owner has a webhook URL configured (`/settings`, per-user —
/// see `set_webhook_url`). Best-effort, same as `generate_thumbnail`: a
/// missing or unreachable webhook is logged, never surfaced to the
/// uploader. Sends both `content` (what Discord's webhook API reads) and
/// `text` (what Slack's reads) with the same message, so one payload works
/// for either without the user having to say which service this is.
///
/// The URL is whatever the account owner typed in — same trust boundary as
/// any other per-user setting in `ravyn` (a naming scheme, an embed
/// template): the owner can only ever point this at wherever *they* choose
/// to send *their own* upload notifications, the same as configuring an
/// outgoing webhook in any other self-hosted tool.
pub(super) async fn notify_upload_webhook(
    state: &AppState,
    owner_id: UserId,
    file: &File,
    base_url: &str,
) {
    let webhook_url = match state.db.get_webhook_url(owner_id).await {
        Ok(Some(url)) => url,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(%err, "failed to load webhook url");
            return;
        }
    };

    let view_url = format!("{base_url}/v/{}", file.id.0);
    let message = format!(
        "📎 **{}** was just uploaded — {view_url}",
        file.original_name
    );
    let body = serde_json::json!({ "content": message, "text": message });

    if let Err(err) = state.http.post(&webhook_url).json(&body).send().await {
        tracing::warn!(%err, "failed to notify upload webhook");
    }
}

struct UploadOutcome {
    name: String,
    result: Result<Uuid, (StatusCode, String)>,
    /// The id of an existing file this owner already has with identical
    /// content, if any - set independently of `result`'s own success,
    /// since the new upload is still saved either way, this is just a
    /// heads-up.
    duplicate_of: Option<Uuid>,
}

#[derive(serde::Serialize)]
struct UploadOutcomeJson {
    name: String,
    id: Option<Uuid>,
    error: Option<String>,
    duplicate_of: Option<Uuid>,
}

impl From<&UploadOutcome> for UploadOutcomeJson {
    fn from(outcome: &UploadOutcome) -> Self {
        match &outcome.result {
            Ok(id) => UploadOutcomeJson {
                name: outcome.name.clone(),
                id: Some(*id),
                error: None,
                duplicate_of: outcome.duplicate_of,
            },
            Err((_, message)) => UploadOutcomeJson {
                name: outcome.name.clone(),
                id: None,
                error: Some(message.clone()),
                duplicate_of: None,
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
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    if !state.rate_limiters.upload.check(&user.id.0.to_string()) {
        return (StatusCode::TOO_MANY_REQUESTS, "too many uploads, slow down").into_response();
    }

    let base_url = resolve_public_base_url(&state, &headers);
    let mut outcomes = Vec::new();

    // Set by two optional, plain form fields (`compress_format`/
    // `compress_quality`) alongside the file(s) in the same multipart
    // body - the dropzone form places them before its file input so they
    // arrive first, and that's the only ordering `Multipart::next_field`
    // gives any field a chance to be read before the "file" fields that
    // need it, so an API/ShareX client sending its own multipart body
    // needs to do the same.
    //
    // `compress_format` is deliberately a *double* Option: the dropzone's
    // `<select>` always sends this field, defaulting to an empty value
    // for its "off" option, and that has to mean "explicitly off" rather
    // than silently falling back to the instance default - otherwise
    // turning the dropdown to "off" while an admin default is set
    // wouldn't actually do anything. `None` (the field never arrived at
    // all, the shape a bare API/ShareX request without this field takes)
    // is the only case that still falls back to the instance default.
    let mut compress_format: Option<Option<ravyn_core::ImageCompressionFormat>> = None;
    let mut compress_quality = None;

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

        match field.name() {
            Some("compress_format") => {
                if let Ok(value) = field.text().await {
                    compress_format = Some(ravyn_core::ImageCompressionFormat::parse(value.trim()));
                }
                continue;
            }
            Some("compress_quality") => {
                if let Ok(value) = field.text().await {
                    compress_quality = value
                        .trim()
                        .parse::<i64>()
                        .ok()
                        .map(ravyn_core::clamp_quality);
                }
                continue;
            }
            _ => {}
        }

        let name = field.file_name().unwrap_or("upload").to_string();
        let saved =
            save_uploaded_part(&state, user.id, field, compress_format, compress_quality).await;
        let mut duplicate_of = None;
        if let Ok(file) = &saved {
            duplicate_of = state
                .db
                .find_duplicate_for_owner(user.id, &file.sha256, file.id)
                .await
                .unwrap_or_default()
                .map(|existing| existing.id.0);

            let _ = state
                .db
                .log_activity(
                    Some(user.id),
                    &user.username,
                    "uploaded a file",
                    Some(&file.original_name),
                )
                .await;

            // Spawned rather than awaited: a slow or unreachable webhook
            // target must never delay the upload response the way it
            // would if this sat in the same request/response cycle.
            let state = state.clone();
            let owner_id = user.id;
            let file = file.clone();
            let base_url = base_url.clone();
            tokio::spawn(async move {
                notify_upload_webhook(&state, owner_id, &file, &base_url).await;
            });
        }
        let result = saved.map(|file| file.id.0);
        outcomes.push(UploadOutcome {
            name,
            result,
            duplicate_of,
        });
    }

    if outcomes.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    if let [only] = outcomes.as_slice() {
        return match &only.result {
            // `duplicate_of` is a new field alongside `id`, not a
            // replacement for it - a ShareX config extracting `{json:id}`
            // keeps working unchanged, it just never looks at this one.
            Ok(id) => Json(serde_json::json!({ "id": id, "duplicate_of": only.duplicate_of }))
                .into_response(),
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

    let _ = state
        .db
        .log_activity(
            Some(user.id),
            &user.username,
            "deleted a file",
            Some(&file.original_name),
        )
        .await;

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

#[derive(Deserialize)]
pub struct SetFileExpiryRequest {
    preset: String,
}

/// Same "instance default is just a starting point" pattern as naming: the
/// owner can always pick a different preset for one file, including turning
/// auto-delete off entirely (`"never"`) even when the instance default would
/// otherwise apply one.
pub async fn set_file_expiry(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetFileExpiryRequest>,
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

    let Some(preset) = ravyn_core::ExpiryPreset::parse(&body.preset) else {
        return (StatusCode::BAD_REQUEST, "invalid expiry preset").into_response();
    };
    let expires_at = preset
        .to_duration()
        .map(|duration| OffsetDateTime::now_utc() + duration);

    if let Err(err) = state.db.set_file_expiry(file.id, expires_at).await {
        tracing::error!(%err, "failed to set file expiry");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub struct SetFileTagsRequest {
    tags: Vec<String>,
}

/// Full replace, same as `set_file_folder` — simpler than an add/remove API
/// for a feature this small. Tags are trimmed, emptied of blanks, and
/// deduplicated here rather than trusted from the client.
pub async fn set_file_tags(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetFileTagsRequest>,
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

    let mut tags: Vec<String> = Vec::new();
    for tag in body.tags {
        let tag = tag.trim().to_string();
        if !tag.is_empty() && !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    if let Err(err) = state.db.set_file_tags(file.id, tags).await {
        tracing::error!(%err, "failed to set file tags");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Runs forever as a background task (spawned once from `main`), deleting
/// any file whose `expires_at` has passed. A fixed 5-minute interval rather
/// than an env-configurable one: this is an internal implementation detail,
/// not something a self-hosted operator needs to tune, and the coarsest
/// preset (`ExpiryPreset::Minutes5`) already tolerates this much slack.
pub async fn run_expiry_sweep(state: AppState) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(300));
    loop {
        ticker.tick().await;

        let expired = match state.db.list_expired_files().await {
            Ok(files) => files,
            Err(err) => {
                tracing::error!(%err, "failed to list expired files");
                continue;
            }
        };

        for file in expired {
            if let Err(err) = state.storage.delete(&file.storage_key).await {
                tracing::warn!(%err, "failed to delete expired file from storage");
                continue;
            }
            let _ = state.thumbnails.delete(&thumbnail_key(file.id)).await;
            match state.db.delete_file(file.id).await {
                Ok(()) => tracing::info!(file_id = %file.id.0, "deleted expired file"),
                Err(err) => tracing::error!(%err, "failed to delete expired file record"),
            }
        }
    }
}
