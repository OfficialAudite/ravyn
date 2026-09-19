use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use super::FileSummary;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderSummary {
    pub id: String,
    pub name: String,
    pub has_password: bool,
    pub created_at: String,
}

#[server]
pub async fn list_folders() -> Result<Vec<FolderSummary>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = ssr::http_client()
        .get(format!("{}/folders", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("not authenticated"));
    }

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server]
pub async fn create_folder(name: String, password: Option<String>) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = ssr::http_client()
        .post(format!("{}/folders", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "name": name, "password": password }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to create folder"));
    }

    Ok(())
}

#[server]
pub async fn delete_folder(id: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = ssr::http_client()
        .delete(format!("{}/folders/{id}", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to delete folder"));
    }

    Ok(())
}

#[server]
pub async fn set_folder_password(
    id: String,
    password: Option<String>,
) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = ssr::http_client()
        .put(format!("{}/folders/{id}/password", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "password": password }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to set password"));
    }

    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedFolder {
    pub name: String,
    pub files: Vec<FileSummary>,
}

/// Public: no login needed. Used by the `/f/{id}` shared-folder page, which
/// anyone with the link (and the password, if the folder is protected) can
/// view — the folder-level equivalent of a shared file link.
#[server]
pub async fn get_shared_folder(
    id: String,
    password: Option<String>,
) -> Result<SharedFolder, ServerFnError> {
    use crate::server_fns::ssr;

    #[derive(Deserialize)]
    struct RawResponse {
        name: String,
        files: Vec<ssr::ApiFileSummary>,
    }

    let mut request = ssr::http_client().get(format!("{}/folders/{id}/files", ssr::api_base_url()));
    if let Some(password) = &password {
        request = request.query(&[("password", password)]);
    }

    let response = request
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ServerFnError::new("password required"));
    }
    if !response.status().is_success() {
        return Err(ServerFnError::new("folder not found"));
    }

    let raw: RawResponse = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    Ok(SharedFolder {
        name: raw.name,
        files: raw.files.into_iter().map(ssr::into_file_summary).collect(),
    })
}
