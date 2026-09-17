use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatedShortUrl {
    pub slug: String,
    pub short_url: String,
}

#[server]
pub async fn create_short_url(destination: String) -> Result<CreatedShortUrl, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .post(format!("{}/short-urls", ssr::api_base_url()))
        .header("Cookie", cookie)
        .json(&serde_json::json!({ "destination": destination }))
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        let message = response.text().await.unwrap_or_default();
        return Err(ServerFnError::new(if message.is_empty() {
            "failed to shorten url".to_string()
        } else {
            message
        }));
    }

    #[derive(Deserialize)]
    struct CreateResponse {
        slug: String,
    }

    let body: CreateResponse = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    let base = ssr::public_api_base_url();
    Ok(CreatedShortUrl {
        short_url: format!("{base}/s/{}", body.slug),
        slug: body.slug,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShortUrlInfo {
    pub id: String,
    pub slug: String,
    pub destination: String,
    pub clicks: i64,
    pub created_at: String,
    pub short_url: String,
}

#[server]
pub async fn list_short_urls() -> Result<Vec<ShortUrlInfo>, ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .get(format!("{}/short-urls", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("not authenticated"));
    }

    #[derive(Deserialize)]
    struct RawShortUrl {
        id: String,
        slug: String,
        destination: String,
        clicks: i64,
        created_at: String,
    }

    let raw: Vec<RawShortUrl> = response
        .json()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    let base = ssr::public_api_base_url();
    Ok(raw
        .into_iter()
        .map(|r| ShortUrlInfo {
            short_url: format!("{base}/s/{}", r.slug),
            id: r.id,
            slug: r.slug,
            destination: r.destination,
            clicks: r.clicks,
            created_at: r.created_at,
        })
        .collect())
}

#[server]
pub async fn delete_short_url(id: String) -> Result<(), ServerFnError> {
    use crate::server_fns::ssr;

    let cookie = ssr::incoming_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("not authenticated"))?;

    let response = reqwest::Client::new()
        .delete(format!("{}/short-urls/{id}", ssr::api_base_url()))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?;

    if !response.status().is_success() {
        return Err(ServerFnError::new("failed to delete short url"));
    }

    Ok(())
}
