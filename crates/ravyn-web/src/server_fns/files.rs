use leptos::prelude::*;

use super::FileSummary;

#[server]
pub async fn list_files() -> Result<Vec<FileSummary>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/files", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("not authenticated"));
    }

    let files: Vec<ssr::ApiFileSummary> = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    // Thumbnails go through `ravyn-web`'s own `/preview/{id}` proxy rather
    // than the direct `ravyn-api` URL `into_file_summary` fills in by
    // default — see `preview_proxy` in `main.rs` for why: a plain `<img>`
    // pointed straight at `ravyn-api` wouldn't carry the session cookie
    // your own password-protected files need.
    Ok(files
        .into_iter()
        .map(ssr::into_file_summary)
        .map(|mut file| {
            file.thumbnail_url = format!("/preview/{}", file.id);
            file
        })
        .collect())
}

#[server]
pub async fn delete_file(id: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .delete(format!("{}/files/{id}", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to delete file"));
    }

    Ok(())
}

#[server]
pub async fn move_file_to_folder(
    id: String,
    folder_id: Option<String>,
) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/files/{id}/folder", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "folder_id": folder_id }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to move file"));
    }

    Ok(())
}

#[server]
pub async fn set_file_password(id: String, password: Option<String>) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/files/{id}/password", ssr::api_base_url()))
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
