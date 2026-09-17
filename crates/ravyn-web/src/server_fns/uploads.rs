use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkedUploadInit {
    pub upload_id: String,
    pub chunk_size: u32,
}

#[server]
pub async fn init_chunked_upload(
    original_name: String,
    content_type: String,
    total_size: i64,
) -> Result<ChunkedUploadInit, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .post(format!("{}/uploads", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({
            "original_name": original_name,
            "content_type": content_type,
            "total_size": total_size,
        }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(ServerFnError::new(if message.is_empty() {
            "failed to start upload".to_string()
        } else {
            message
        }));
    }

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkedUploadStatus {
    pub received_bytes: i64,
    pub total_size: i64,
    pub parts: Vec<i32>,
}

#[server]
pub async fn get_chunked_upload_status(
    upload_id: String,
) -> Result<ChunkedUploadStatus, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/uploads/{upload_id}", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("upload not found"));
    }

    response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

/// Returns the finished file's id, same as a regular single-request
/// upload would - the dropzone treats both paths' results the same way
/// once this resolves.
#[server]
pub async fn complete_chunked_upload(upload_id: String) -> Result<String, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/uploads/{upload_id}/complete",
            ssr::api_base_url()
        ))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(ServerFnError::new(if message.is_empty() {
            "failed to complete upload".to_string()
        } else {
            message
        }));
    }

    #[derive(Deserialize)]
    struct CompleteResponse {
        id: String,
    }

    let body: CompleteResponse = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;
    Ok(body.id)
}

#[server]
pub async fn cancel_chunked_upload(upload_id: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .delete(format!("{}/uploads/{upload_id}", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to cancel upload"));
    }

    Ok(())
}
