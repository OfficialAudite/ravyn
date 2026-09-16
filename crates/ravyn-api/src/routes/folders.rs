use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use ravyn_core::{auth as core_auth, Folder, FolderId, User};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{hash_optional_password, FileSummary};
use crate::{auth::AuthedUser, state::AppState};

#[derive(Deserialize)]
pub struct CreateFolderRequest {
    name: String,
    password: Option<String>,
}

pub async fn create_folder(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<CreateFolderRequest>,
) -> Response {
    let password_hash = match hash_optional_password(body.password) {
        Ok(hash) => hash,
        Err(status) => return status.into_response(),
    };

    let folder = Folder {
        id: FolderId::new(),
        owner_id: user.id,
        name: body.name,
        password_hash,
        created_at: OffsetDateTime::now_utc(),
    };

    if let Err(err) = state.db.create_folder(&folder).await {
        tracing::error!(%err, "failed to create folder");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(serde_json::json!({ "id": folder.id.0 })).into_response()
}

#[derive(Serialize)]
pub struct FolderSummary {
    id: Uuid,
    name: String,
    has_password: bool,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

impl From<Folder> for FolderSummary {
    fn from(folder: Folder) -> Self {
        FolderSummary {
            id: folder.id.0,
            name: folder.name,
            has_password: folder.password_hash.is_some(),
            created_at: folder.created_at,
        }
    }
}

pub async fn list_folders(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    let folders = match state.db.list_folders_for_owner(user.id).await {
        Ok(folders) => folders,
        Err(err) => {
            tracing::error!(%err, "failed to list folders");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let summaries: Vec<FolderSummary> = folders.into_iter().map(FolderSummary::from).collect();
    Json(summaries).into_response()
}

pub async fn delete_folder(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    let folder = match state.db.get_folder(FolderId(id)).await {
        Ok(Some(folder)) => folder,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up folder");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if folder.owner_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Files inside aren't deleted — `folder_id` just goes back to null
    // (ON DELETE SET NULL). A folder is an organizational label, not a
    // container the files' lifetime depends on.
    if let Err(err) = state.db.delete_folder(folder.id).await {
        tracing::error!(%err, "failed to delete folder");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub struct SetPasswordRequest {
    password: Option<String>,
}

pub async fn set_folder_password(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetPasswordRequest>,
) -> Response {
    let folder = match state.db.get_folder(FolderId(id)).await {
        Ok(Some(folder)) => folder,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up folder");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if folder.owner_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let hash = match hash_optional_password(body.password) {
        Ok(hash) => hash,
        Err(status) => return status.into_response(),
    };

    if let Err(err) = state.db.set_folder_password(folder.id, hash).await {
        tracing::error!(%err, "failed to set folder password");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub struct FolderAccessQuery {
    password: Option<String>,
}

#[derive(Serialize)]
pub struct FolderFilesResponse {
    name: String,
    files: Vec<FileSummary>,
}

/// Public: anyone with the link (and the password, if one is set) can view
/// a folder's files — the same sharing model as an individual file link.
pub async fn get_folder_files(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<FolderAccessQuery>,
    requester: Option<AuthedUser>,
) -> Response {
    let folder = match state.db.get_folder(FolderId(id)).await {
        Ok(Some(folder)) => folder,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up folder");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let requester = requester.map(|AuthedUser(user)| user);
    if !is_folder_authorized(&folder, requester.as_ref(), query.password.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let files = match state.db.list_files_for_folder(folder.id).await {
        Ok(files) => files,
        Err(err) => {
            tracing::error!(%err, "failed to list folder files");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(FolderFilesResponse {
        name: folder.name,
        files: files.into_iter().map(FileSummary::from).collect(),
    })
    .into_response()
}

fn is_folder_authorized(folder: &Folder, requester: Option<&User>, password: Option<&str>) -> bool {
    if let Some(user) = requester {
        if user.id == folder.owner_id {
            return true;
        }
    }

    match &folder.password_hash {
        None => true,
        Some(hash) => password
            .map(|candidate| core_auth::verify_password(candidate, hash))
            .unwrap_or(false),
    }
}
