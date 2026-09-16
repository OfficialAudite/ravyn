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
pub struct AccountInfo {
    pub username: String,
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
pub async fn create_api_token(name: String) -> Result<String, ServerFnError> {
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

    Ok(body.token)
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
