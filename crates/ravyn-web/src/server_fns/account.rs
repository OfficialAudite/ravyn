use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[server]
pub async fn login(username: String, password: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let response = reqwest::Client::new()
        .post(format!("{}/login", ssr::api_base_url()))
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("wrong username or password"));
    }

    ssr::relay_set_cookie(&response);
    Ok(())
}

#[server]
pub async fn logout() -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    if let Some(cookie) = ssr::incoming_cookie().await {
        let _ = reqwest::Client::new()
            .post(format!("{}/logout", ssr::api_base_url()))
            .header("Cookie", cookie)
            .send()
            .await;
    }

    ssr::clear_session_cookie();
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistrationStatus {
    /// No account exists yet — the login screen should offer to set one up
    /// instead of a login form.
    pub setup_required: bool,
    /// "closed" | "open" | "invite" — irrelevant while `setup_required`, but
    /// always reported so the register form knows whether to ask for a code.
    pub mode: String,
}

#[server]
pub async fn get_registration_status() -> Result<RegistrationStatus, ServerFnError> {
    use crate::server_fns::ssr;

    let response = reqwest::Client::new()
        .get(format!("{}/registration-status", ssr::api_base_url()))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server]
pub async fn register(
    username: String,
    password: String,
    invite_token: Option<String>,
) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let response = reqwest::Client::new()
        .post(format!("{}/register", ssr::api_base_url()))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
            "invite_token": invite_token,
        }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(ServerFnError::new(if message.is_empty() {
            "registration failed".to_string()
        } else {
            message
        }));
    }

    ssr::relay_set_cookie(&response);
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountInfo {
    pub username: String,
    pub is_admin: bool,
}

#[server]
pub async fn me() -> Result<AccountInfo, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/me", ssr::api_base_url()))
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatedApiToken {
    pub token: String,
    /// A ready-to-import ShareX custom uploader config (the same shape as
    /// `contrib/sharex/ravyn.sxcu`), with this token and the instance's
    /// public URL already filled in.
    pub sharex_config: String,
}

#[server]
pub async fn create_api_token(name: String) -> Result<CreatedApiToken, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .post(format!("{}/api-tokens", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to create token"));
    }

    #[derive(Deserialize)]
    struct TokenResponse {
        token: String,
    }

    let body: TokenResponse = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    let base = ssr::public_api_base_url();
    let sharex_config = serde_json::json!({
        "Version": "17.0.0",
        "Name": "ravyn",
        "DestinationType": "ImageUploader, FileUploader",
        "RequestMethod": "POST",
        "RequestURL": format!("{base}/files"),
        "Headers": { "Authorization": format!("Bearer {}", body.token) },
        "Body": "MultipartFormData",
        "FileFormName": "file",
        "URL": format!("{base}/v/{{json:id}}"),
    })
    .to_string();

    Ok(CreatedApiToken {
        token: body.token,
        sharex_config,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiTokenInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[server]
pub async fn list_api_tokens() -> Result<Vec<ApiTokenInfo>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/api-tokens", ssr::api_base_url()))
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
pub async fn delete_api_token(id: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .delete(format!("{}/api-tokens/{id}", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to revoke token"));
    }

    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EmbedSettings {
    pub enabled: bool,
    pub title: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub site_name: Option<String>,
}

#[server]
pub async fn get_embed_settings() -> Result<EmbedSettings, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/embed-settings", ssr::api_base_url()))
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
pub async fn set_embed_settings(settings: EmbedSettings) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/embed-settings", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&settings)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to save embed settings"));
    }

    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageInfo {
    pub backend: String,
    pub bucket: Option<String>,
    pub endpoint: Option<String>,
    pub root: Option<String>,
}

#[server]
pub async fn get_storage_info() -> Result<StorageInfo, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/storage-info", ssr::api_base_url()))
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstanceSettings {
    pub registration_mode: String,
}

#[server]
pub async fn get_instance_settings() -> Result<InstanceSettings, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/instance-settings", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("not authorized"));
    }

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server]
pub async fn set_instance_settings(registration_mode: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/instance-settings", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "registration_mode": registration_mode }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to save registration mode"));
    }

    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatedInvite {
    pub token: String,
}

#[server]
pub async fn create_invite() -> Result<CreatedInvite, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .post(format!("{}/invites", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to create invite"));
    }

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteInfo {
    pub id: String,
    pub created_at: String,
    pub used: bool,
}

#[server]
pub async fn list_invites() -> Result<Vec<InviteInfo>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/invites", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("not authorized"));
    }

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server]
pub async fn delete_invite(id: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .delete(format!("{}/invites/{id}", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to revoke invite"));
    }

    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdminUserInfo {
    pub id: String,
    pub username: String,
    pub is_admin: bool,
    pub created_at: String,
    pub storage_used_bytes: i64,
    pub max_storage_bytes: Option<i64>,
}

#[server]
pub async fn list_users() -> Result<Vec<AdminUserInfo>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/admin/users", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("not authorized"));
    }

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server]
pub async fn set_user_limit(
    id: String,
    max_storage_bytes: Option<i64>,
) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/admin/users/{id}/limit", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "max_storage_bytes": max_storage_bytes }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to save storage limit"));
    }

    Ok(())
}
