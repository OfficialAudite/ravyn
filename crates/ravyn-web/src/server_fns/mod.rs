mod account;
mod files;
mod folders;

pub use account::*;
pub use files::*;
pub use folders::*;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileSummary {
    pub id: String,
    pub original_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub folder_id: Option<String>,
    pub has_password: bool,
    pub created_at: String,
    /// The shareable link, e.g. `http://localhost:3000/v/<id>` — copy-link
    /// and "open" both use this. It's `ravyn-api`'s `/v/{id}`, not the raw
    /// `/files/{id}`: when the owner has embeds off it just redirects
    /// straight to the raw file (so Discord/etc. preview it exactly as
    /// before), and when embeds are on it's the page that actually carries
    /// the Open Graph tags — a raw file URL has nowhere to put those.
    /// Deliberately not `/api/v/<id>` on `ravyn-web`'s own origin either —
    /// that prefix is where Leptos mounts server functions.
    pub url: String,
    /// A small local-disk preview URL. Only meaningful for images — for
    /// anything else, or if generation failed, this 404s and the UI falls
    /// back to a file-type icon.
    pub thumbnail_url: String,
    /// The original bytes, straight from `ravyn-api`'s `/files/{id}` — what
    /// the file detail modal plays/displays inline, downloads, and opens in
    /// a new tab. Defaults to a direct `ravyn-api` URL (fine for the public
    /// shared-folder page, which never overrides this); `list_files`
    /// overrides it to `ravyn-web`'s own `/raw/{id}` proxy, the same
    /// cookie-forwarding trick `thumbnail_url` already uses, so the owner's
    /// own password-protected files preview correctly in their own
    /// dashboard instead of silently failing to load.
    pub raw_url: String,
}

/// Everything here talks to `ravyn-api` server-to-server, forwarding the
/// browser's session cookie by hand. This is what lets `ravyn-web` and
/// `ravyn-api` live on different origins without any CORS setup — the
/// browser only ever talks to `ravyn-web`.
#[cfg(feature = "ssr")]
pub(crate) mod ssr {
    use axum::http::{header, HeaderMap, HeaderValue};
    use leptos::prelude::*;
    use leptos_axum::ResponseOptions;
    use serde::Deserialize;

    use super::FileSummary;

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

    #[derive(Deserialize)]
    pub struct ApiFileSummary {
        pub id: String,
        pub original_name: String,
        pub content_type: String,
        pub size_bytes: u64,
        pub sha256: String,
        pub folder_id: Option<String>,
        pub has_password: bool,
        pub created_at: String,
    }

    /// Fills in the direct-to-`ravyn-api` URLs. Shared by every server
    /// function that returns files, so the URL scheme only lives in one place.
    pub fn into_file_summary(file: ApiFileSummary) -> FileSummary {
        let base = public_api_base_url();
        FileSummary {
            url: format!("{base}/v/{}", file.id),
            thumbnail_url: format!("{base}/files/{}/thumbnail", file.id),
            raw_url: format!("{base}/files/{}", file.id),
            id: file.id,
            original_name: file.original_name,
            content_type: file.content_type,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
            folder_id: file.folder_id,
            has_password: file.has_password,
            created_at: file.created_at,
        }
    }
}
