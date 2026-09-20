// See the matching attribute in lib.rs for why.
#![recursion_limit = "256"]

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::{
        extract::DefaultBodyLimit,
        http::header::{HeaderValue, CACHE_CONTROL},
        routing::{get, patch, post},
        Router,
    };
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use ravyn_web::app::{shell, App};
    use tower_http::set_header::SetResponseHeaderLayer;

    // See the matching comment in ravyn-api's main.rs.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    let app = Router::new()
        .route("/health", get(health))
        .route(
            "/api/*fn_name",
            get(leptos_axum::handle_server_fns).post(leptos_axum::handle_server_fns),
        )
        .route("/upload", post(upload_proxy))
        .route("/upload-chunk/:id/:part_number", patch(upload_chunk_proxy))
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
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

/// See the matching function in ravyn-api's main.rs for why this exists.
#[cfg(feature = "ssr")]
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, draining in-flight requests");
}

/// A readiness check, not just "the process is bound to a port": this
/// service is only actually useful if it can reach `ravyn-api` too, since
/// every server function and proxy route depends on it. Bounded by a
/// short timeout so a hung upstream makes this fail fast rather than
/// hang right along with it.
#[cfg(feature = "ssr")]
async fn health() -> axum::response::Response {
    use axum::response::IntoResponse;

    let api_base =
        std::env::var("RAVYN_API_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

    let request = reqwest::Client::new()
        .get(format!("{api_base}/health"))
        .timeout(std::time::Duration::from_secs(3))
        .send();

    match request.await {
        Ok(response) if response.status().is_success() => "ok".into_response(),
        Ok(response) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            format!("api returned {}", response.status()),
        )
            .into_response(),
        Err(err) => {
            tracing::error!(%err, "health check: api unreachable");
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "api unreachable",
            )
                .into_response()
        }
    }
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
///
/// Streams the body through rather than buffering it: taking `body` as
/// `axum::body::Body` (a stream) instead of `axum::body::Bytes` (which
/// axum would fully materialize before this handler even runs) means this
/// server never holds more than one chunk of the upload in memory, no
/// matter how large it is — the same fix `ravyn-api`'s own multipart
/// handling needed, and for the same reason: this server sits in front of
/// that one, so buffering here would have defeated buffering being fixed
/// there.
#[cfg(feature = "ssr")]
async fn upload_proxy(
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let api_base =
        std::env::var("RAVYN_API_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

    let mut request = reqwest::Client::new()
        .post(format!("{api_base}/files"))
        .body(reqwest::Body::wrap_stream(body.into_data_stream()));

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
            // `/files`' response carries a `duplicate_of` id (single-file
            // shape: `{"id":..,"duplicate_of":..}`, multi-file: an array of
            // that shape) whenever the upload turned out to be
            // byte-identical to a file this account already had. Riding it
            // along on the redirect's query string, rather than reading it
            // client-side, means the notice on `/` works the same whether
            // or not JS ever ran - same reason this whole path is a plain
            // form POST and not a fetch call to begin with.
            let duplicate_of = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| {
                    body.get("duplicate_of")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                        .or_else(|| {
                            body.as_array()?.iter().find_map(|entry| {
                                entry
                                    .get("duplicate_of")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string)
                            })
                        })
                });

            match duplicate_of {
                Some(id) => {
                    axum::response::Redirect::to(&format!("/?duplicate_of={id}")).into_response()
                }
                None => axum::response::Redirect::to("/").into_response(),
            }
        }
        _ => (axum::http::StatusCode::BAD_GATEWAY, "upload failed").into_response(),
    }
}

/// The chunked-upload counterpart to `upload_proxy`, for the same reason:
/// a chunk's bytes need to stream straight through rather than pass
/// through a Leptos server function's own encoding. Unlike `upload_proxy`,
/// this is called from JS (`browser::upload_large_file`), not a plain HTML
/// form submit, so it relays the JSON body back rather than redirecting -
/// the caller needs to read `received_bytes` out of it.
#[cfg(feature = "ssr")]
async fn upload_chunk_proxy(
    axum::extract::Path((id, part_number)): axum::extract::Path<(String, i32)>,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let api_base =
        std::env::var("RAVYN_API_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

    let mut request = reqwest::Client::new()
        .patch(format!("{api_base}/uploads/{id}/{part_number}"))
        .body(reqwest::Body::wrap_stream(body.into_data_stream()));

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
    let bytes = response.bytes().await.unwrap_or_default();
    (status, bytes).into_response()
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

    // Forwarded as-is rather than re-decided here: `ravyn-api`'s `/files/{id}`
    // already picked these based on the file's content type (nosniff always,
    // plus a forced download for anything unsafe to open as a top-level
    // navigation, e.g. an uploaded SVG or HTML file) - this proxy just needs
    // to not silently drop them the way only forwarding Content-Type did.
    let mut headers = axum::http::HeaderMap::new();
    for name in [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::CONTENT_DISPOSITION,
    ] {
        if let Some(value) = response.headers().get(&name) {
            headers.insert(name, value.clone());
        }
    }
    if let Some(value) = response.headers().get("x-content-type-options") {
        headers.insert("x-content-type-options", value.clone());
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    (status, headers, body).into_response()
}

#[cfg(not(feature = "ssr"))]
fn main() {}
