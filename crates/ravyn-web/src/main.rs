#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::{
        routing::{get, post},
        Router,
    };
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use ravyn_web::app::{shell, App};

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
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("ravyn-web listening on {addr}");
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
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

#[cfg(not(feature = "ssr"))]
fn main() {}
