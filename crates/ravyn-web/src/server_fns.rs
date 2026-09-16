use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileSummary {
    pub id: String,
    pub original_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub created_at: String,
    /// The publicly-reachable download URL, e.g. `http://localhost:3000/files/<id>`.
    /// Deliberately not `/api/files/<id>` on `ravyn-web`'s own origin — that
    /// path is where Leptos mounts server functions, so it would collide.
    pub url: String,
}

/// Everything here talks to `ravyn-api` server-to-server, forwarding the
/// browser's session cookie by hand. This is what lets `ravyn-web` and
/// `ravyn-api` live on different origins without any CORS setup — the
/// browser only ever talks to `ravyn-web`.
#[cfg(feature = "ssr")]
mod ssr {
    use axum::http::{header, HeaderMap, HeaderValue};
    use leptos::prelude::*;
    use leptos_axum::ResponseOptions;

    /// Where `ravyn-web` reaches `ravyn-api` itself (e.g. `http://api:3000`
    /// inside docker-compose — not reachable from the browser).
    pub fn api_base_url() -> String {
        std::env::var("RAVYN_API_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into())
    }

    /// Where the *browser* can reach `ravyn-api` directly, for public file
    /// links. Falls back to `api_base_url()`, which is correct whenever
    /// both run on localhost (e.g. plain `cargo leptos watch`), but must be
    /// set explicitly whenever the two hostnames differ, as they do across
    /// a docker-compose network.
    pub fn public_api_base_url() -> String {
        std::env::var("RAVYN_PUBLIC_API_URL").unwrap_or_else(|_| api_base_url())
    }

    pub async fn incoming_cookie() -> Option<String> {
        let headers: HeaderMap = leptos_axum::extract().await.ok()?;
        headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    }

    /// Copies a `Set-Cookie` header from an `ravyn-api` response onto the
    /// response `ravyn-web` sends back to the browser.
    pub fn relay_set_cookie(response: &reqwest::Response) {
        let Some(value) = response.headers().get(reqwest::header::SET_COOKIE) else {
            return;
        };
        let Ok(value) = HeaderValue::from_bytes(value.as_bytes()) else {
            return;
        };
        expect_context::<ResponseOptions>().insert_header(header::SET_COOKIE, value);
    }

    pub fn clear_session_cookie() {
        expect_context::<ResponseOptions>().insert_header(
            header::SET_COOKIE,
            HeaderValue::from_static("ravyn_session=; Path=/; Max-Age=0"),
        );
    }
}

#[server]
pub async fn list_files() -> Result<Vec<FileSummary>, ServerFnError> {
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

    #[derive(Deserialize)]
    struct ApiFileSummary {
        id: String,
        original_name: String,
        content_type: String,
        size_bytes: u64,
        sha256: String,
        created_at: String,
    }

    let files: Vec<ApiFileSummary> = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    let public_base = ssr::public_api_base_url();
    Ok(files
        .into_iter()
        .map(|file| FileSummary {
            url: format!("{public_base}/files/{}", file.id),
            id: file.id,
            original_name: file.original_name,
            content_type: file.content_type,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
            created_at: file.created_at,
        })
        .collect())
}

#[server]
pub async fn login(username: String, password: String) -> Result<(), ServerFnError> {
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

#[server]
pub async fn delete_file(id: String) -> Result<(), ServerFnError> {
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
pub async fn create_api_token(name: String) -> Result<String, ServerFnError> {
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
