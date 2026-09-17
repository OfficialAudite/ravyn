// See the matching attribute in lib.rs for why.
#![recursion_limit = "256"]

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::{
        extract::DefaultBodyLimit,
        http::header::{HeaderValue, CACHE_CONTROL},
        routing::{get, post},
        Router,
    };
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use ravyn_web::app::{shell, App};
    use tower_http::set_header::SetResponseHeaderLayer;

    tracing_subscriber::fmt::init();

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    let app = Router::new()
        .route(
            "/api/*fn_name",
            get(leptos_axum::handle_server_fns).post(leptos_axum::handle_server_fns),
        )
        .route("/upload", post(upload_proxy))
        .route("/preview/:id", get(preview_proxy))
        .route("/raw/:id", get(raw_proxy))
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .layer(SetResponseHeaderLayer::if_not_present(
            CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .layer(DefaultBodyLimit::max(max_upload_bytes()))
        .with_state(leptos_options);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("ravyn-web listening on {addr}");
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

/// Kept identical to the same-named helper in `ravyn-api` — read the
/// comment there for why this needs its own copy of the same
/// `MAX_UPLOAD_MB` override rather than deferring to the API's own limit:
/// this server receives the whole upload body itself, in `upload_proxy`
/// below, before it ever reaches `ravyn-api`.
#[cfg(feature = "ssr")]
fn max_upload_bytes() -> usize {
    std::env::var("MAX_UPLOAD_MB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2048)
        .saturating_mul(1024 * 1024)
}

/// Proxies a browser's multipart upload straight through to `ravyn-api`,
/// forwarding the session cookie unchanged. Kept as a plain form POST
/// (rather than a Leptos server function) so uploads keep working even
/// without JS/hydration.
#[cfg(feature = "ssr")]
async fn upload_proxy(
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let api_base =
        std::env::var("RAVYN_API_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

    let mut request = reqwest::Client::new()
        .post(format!("{api_base}/files"))
        .body(body);

    if let Some(cookie) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
    {
        request = request.header("Cookie", cookie);
    }
    if let Some(content_type) = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        request = request.header("Content-Type", content_type);
    }

    match request.send().await {
        Ok(response) if response.status().is_success() => {
            axum::response::Redirect::to("/").into_response()
        }
        _ => (axum::http::StatusCode::BAD_GATEWAY, "upload failed").into_response(),
    }
}

/// Proxies the dashboard's own thumbnail previews through `ravyn-web`,
/// forwarding the session cookie. Needed because a plain `<img src>`
/// pointing straight at `ravyn-api` would be a cross-origin request that
/// never carries the cookie — that cookie belongs to `ravyn-web`'s origin
/// (see `server_fns::ssr::relay_set_cookie`), not `ravyn-api`'s. Without
/// this, your own password-protected files would show as broken images in
/// your own dashboard. Public links (copy-link, the shared folder page)
/// don't need this — they hit `ravyn-api` directly and rely on its own
/// password gate instead.
#[cfg(feature = "ssr")]
async fn preview_proxy(
    axum::extract::Path(id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let api_base =
        std::env::var("RAVYN_API_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

    let mut request = reqwest::Client::new().get(format!("{api_base}/files/{id}/thumbnail"));
    if let Some(cookie) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
    {
        request = request.header("Cookie", cookie);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => return axum::http::StatusCode::BAD_GATEWAY.into_response(),
    };

    let status = axum::http::StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .cloned();
    let bytes = response.bytes().await.unwrap_or_default();

    let mut headers = axum::http::HeaderMap::new();
    if let Some(content_type) = content_type {
        headers.insert(axum::http::header::CONTENT_TYPE, content_type);
    }

    (status, headers, bytes).into_response()
}

/// The same cookie-forwarding problem `preview_proxy` solves for
/// thumbnails, but for the original file — needed by the file detail
/// modal's inline preview (`<img>`/`<video>`/`<audio>`), which otherwise
/// hits `ravyn-api` directly and silently fails to load for the owner's
/// own password-protected files (no error shown, just a broken preview —
/// the metadata panel next to it still renders fine, which is what makes
/// this particular failure confusing rather than obviously broken).
/// Streams the body through rather than buffering it fully in memory like
/// `preview_proxy` does for thumbnails — a thumbnail is a few KB, but an
/// original file can be arbitrarily large.
#[cfg(feature = "ssr")]
async fn raw_proxy(
    axum::extract::Path(id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let api_base =
        std::env::var("RAVYN_API_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

    let mut request = reqwest::Client::new().get(format!("{api_base}/files/{id}"));
    if let Some(cookie) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
    {
        request = request.header("Cookie", cookie);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => return axum::http::StatusCode::BAD_GATEWAY.into_response(),
    };

    let status = axum::http::StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .cloned();

    let mut headers = axum::http::HeaderMap::new();
    if let Some(content_type) = content_type {
        headers.insert(axum::http::header::CONTENT_TYPE, content_type);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    (status, headers, body).into_response()
}

#[cfg(not(feature = "ssr"))]
fn main() {}
