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

    // Thumbnails and the file detail modal's inline preview both go
    // through `ravyn-web`'s own cookie-forwarding proxies rather than the
    // direct `ravyn-api` URLs `into_file_summary` fills in by default — see
    // `preview_proxy`/`raw_proxy` in `main.rs` for why: a plain
    // `<img>`/`<video>` pointed straight at `ravyn-api` wouldn't carry the
    // session cookie your own password-protected files need.
    Ok(files
        .into_iter()
        .map(ssr::into_file_summary)
        .map(|mut file| {
            file.thumbnail_url = format!("/preview/{}", file.id);
            file.raw_url = format!("/raw/{}", file.id);
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

#[server]
pub async fn rename_file(id: String, name: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/files/{id}/name", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to rename file"));
    }

    Ok(())
}
