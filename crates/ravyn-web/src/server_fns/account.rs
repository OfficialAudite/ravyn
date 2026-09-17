use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LoginResult {
    /// Session cookie already set — nothing further to do.
    Success,
    /// A correct password on a 2FA account doesn't sign you in by itself —
    /// `login_token` identifies the pending login for `login_totp`.
    TotpRequired { login_token: String },
}

#[server]
pub async fn login(username: String, password: String) -> Result<LoginResult, ServerFnError> {
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

    // Set for the plain-session case; a no-op for the totp-required case,
    // since `/login` never sets a cookie until `/login/totp` confirms a code.
    ssr::relay_set_cookie(&response);

    #[derive(Deserialize)]
    struct TotpRequiredBody {
        totp_required: bool,
        login_token: String,
    }

    // A successful plain login's body is empty, which fails to parse here —
    // that failure is exactly how the two cases are told apart.
    match response.json::<TotpRequiredBody>().await {
        Ok(body) if body.totp_required => Ok(LoginResult::TotpRequired {
            login_token: body.login_token,
        }),
        _ => Ok(LoginResult::Success),
    }
}

#[server]
pub async fn login_totp(login_token: String, code: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let response = reqwest::Client::new()
        .post(format!("{}/login/totp", ssr::api_base_url()))
        .json(&serde_json::json!({ "login_token": login_token, "code": code }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("invalid code"));
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
    pub totp_enabled: bool,
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

#[server]
pub async fn change_password(
    current_password: String,
    new_password: String,
) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/me/password", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({
            "current_password": current_password,
            "new_password": new_password,
        }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ServerFnError::new("current password is incorrect"));
    }
    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to change password"));
    }

    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TotpSetup {
    pub secret: String,
    pub otpauth_url: String,
    pub qr_code_base64: String,
}

#[server]
pub async fn setup_totp() -> Result<TotpSetup, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .post(format!("{}/me/totp/setup", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(ServerFnError::new(if message.is_empty() {
            "failed to start 2FA setup".to_string()
        } else {
            message
        }));
    }

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server]
pub async fn confirm_totp(code: String) -> Result<Vec<String>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .post(format!("{}/me/totp/confirm", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("invalid code"));
    }

    #[derive(Deserialize)]
    struct ConfirmResponse {
        recovery_codes: Vec<String>,
    }

    let body: ConfirmResponse = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;
    Ok(body.recovery_codes)
}

#[server]
pub async fn disable_totp(password: String, code: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .post(format!("{}/me/totp/disable", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "password": password, "code": code }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(ServerFnError::new(if message.is_empty() {
            "failed to disable 2FA".to_string()
        } else {
            message
        }));
    }

    Ok(())
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

#[server]
pub async fn get_webhook_url() -> Result<Option<String>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/webhook-settings", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("not authenticated"));
    }

    #[derive(Deserialize)]
    struct WebhookSettings {
        webhook_url: Option<String>,
    }

    let body: WebhookSettings = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;
    Ok(body.webhook_url)
}

#[server]
pub async fn set_webhook_url(webhook_url: Option<String>) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/webhook-settings", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "webhook_url": webhook_url }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(ServerFnError::new(if message.is_empty() {
            "failed to save webhook url".to_string()
        } else {
            message
        }));
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
    pub naming_scheme: String,
    pub random_name_length: i64,
    pub default_expiry_preset: String,
    pub strip_exif: bool,
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

/// Every argument optional so the registration form, the naming-scheme
/// form, the auto-delete form, and the EXIF-stripping toggle (independent
/// forms on the same admin page) can each save just their own setting
/// without clobbering the others' — mirrors `SetInstanceSettingsRequest` on
/// the API side.
#[server]
pub async fn set_instance_settings(
    registration_mode: Option<String>,
    naming_scheme: Option<String>,
    random_name_length: Option<i64>,
    default_expiry_preset: Option<String>,
    strip_exif: Option<bool>,
) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .put(format!("{}/instance-settings", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({
            "registration_mode": registration_mode,
            "naming_scheme": naming_scheme,
            "random_name_length": random_name_length,
            "default_expiry_preset": default_expiry_preset,
            "strip_exif": strip_exif,
        }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to save instance settings"));
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
    pub file_count: i64,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TypeCounts {
    pub images: i64,
    pub videos: i64,
    pub audio: i64,
    pub documents: i64,
    pub other: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyStats {
    pub file_count: i64,
    pub storage_used_bytes: i64,
    pub max_storage_bytes: Option<i64>,
    #[serde(flatten)]
    pub by_type: TypeCounts,
}

#[server]
pub async fn get_my_stats() -> Result<MyStats, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/me/stats", ssr::api_base_url()))
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
pub struct InstanceStats {
    pub total_users: i64,
    pub total_files: i64,
    pub total_storage_bytes: i64,
    #[serde(flatten)]
    pub by_type: TypeCounts,
}

#[server]
pub async fn get_admin_stats() -> Result<InstanceStats, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/admin/stats", ssr::api_base_url()))
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityLogEntry {
    pub id: String,
    pub username: String,
    pub action: String,
    pub target: Option<String>,
    pub created_at: String,
}

#[server]
pub async fn list_activity() -> Result<Vec<ActivityLogEntry>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/admin/activity", ssr::api_base_url()))
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
