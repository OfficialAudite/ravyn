mod auth;
mod rate_limit;
mod routes;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use rate_limit::RateLimiters;
use ravyn_core::{auth as core_auth, User, UserId};
use ravyn_db::Db;
use ravyn_storage::{Storage, StorageConfig};
use state::AppState;
use time::OffsetDateTime;

#[tokio::main]
async fn main() {
    // Unfiltered by default (`fmt::init()`) means every request-level
    // `tracing::info!` sticks around in `docker logs` forever - fine for
    // a quiet self-hosted instance, a lot less fine after months of
    // continuous uptime with no rotation. `RUST_LOG=warn` (or `error`)
    // trims that down without a redeploy.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let args: Vec<String> = std::env::args().collect();
    if let [_, cmd, username, password] = args.as_slice() {
        if cmd == "create-user" {
            create_user(&database_url, username, password).await;
            return;
        }
    }

    let db = Db::connect(&database_url)
        .await
        .expect("failed to connect to database");
    db.migrate().await.expect("failed to run migrations");

    let storage = Storage::new(main_storage_config()).expect("failed to initialize storage");

    let thumbnail_root = std::env::var("THUMBNAIL_ROOT").unwrap_or_else(|_| "./thumbnails".into());
    let thumbnails = Storage::new(StorageConfig::Local {
        root: thumbnail_root,
    })
    .expect("failed to initialize thumbnail storage");

    let state = AppState {
        db,
        storage,
        thumbnails,
        // A timeout matters here specifically because the client posts to
        // a URL the account owner supplied (a webhook endpoint) — without
        // one, a target that never responds would leak a spawned task per
        // upload indefinitely.
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build http client"),
        public_url: std::env::var("RAVYN_PUBLIC_API_URL").ok(),
        rate_limiters: Arc::new(RateLimiters::default()),
    };

    tokio::spawn(routes::run_expiry_sweep(state.clone()));
    tokio::spawn(routes::run_chunked_upload_sweep(state.clone()));
    tokio::spawn(rate_limit::run_rate_limit_cleanup(
        state.rate_limiters.clone(),
    ));

    let app = routes::router(state);

    let addr: SocketAddr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()
        .expect("invalid LISTEN_ADDR");

    tracing::info!("listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

/// Resolves once Docker (or Ctrl+C locally) asks this process to stop.
/// Without it, `axum::serve` never stops accepting new connections on its
/// own, so a `docker stop`/redeploy sends SIGTERM straight to the process
/// with nothing listening for it - the OS default for an unhandled
/// SIGTERM is to kill it immediately, cutting off whatever request (an
/// in-progress upload, most likely) happened to be mid-flight. Passing
/// this to `with_graceful_shutdown` instead makes axum stop accepting new
/// connections the moment the signal arrives but let already-in-flight
/// ones finish, up to however long Docker's own stop timeout allows
/// before it escalates to SIGKILL.
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

/// Chooses where uploaded files themselves are stored. `STORAGE_BACKEND=s3`
/// switches to S3 (or any S3-compatible service — set `S3_ENDPOINT` for
/// R2, MinIO, etc.); anything else (including unset) stores on local disk
/// under `STORAGE_ROOT`. Thumbnails are handled separately and always stay
/// on local disk — see `main()`.
fn main_storage_config() -> StorageConfig {
    match std::env::var("STORAGE_BACKEND").as_deref() {
        Ok("s3") => StorageConfig::S3 {
            bucket: std::env::var("S3_BUCKET")
                .expect("S3_BUCKET must be set when STORAGE_BACKEND=s3"),
            region: std::env::var("S3_REGION").unwrap_or_else(|_| "auto".into()),
            endpoint: std::env::var("S3_ENDPOINT").ok(),
            access_key_id: std::env::var("S3_ACCESS_KEY_ID")
                .expect("S3_ACCESS_KEY_ID must be set when STORAGE_BACKEND=s3"),
            secret_access_key: std::env::var("S3_SECRET_ACCESS_KEY")
                .expect("S3_SECRET_ACCESS_KEY must be set when STORAGE_BACKEND=s3"),
        },
        _ => StorageConfig::Local {
            root: std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./data".into()),
        },
    }
}

/// Creates an admin account from the CLI: `cargo run -p ravyn-api --
/// create-user <username> <password>`. Mainly useful for bootstrapping an
/// instance without ever exposing it publicly, or for adding another admin
/// later — the normal way to get a first account is registering through the
/// web UI, which mints an admin for you once (see `routes::account::register`)
/// and lets that admin decide from `/settings` how anyone after them signs up.
async fn create_user(database_url: &str, username: &str, password: &str) {
    let db = Db::connect(database_url)
        .await
        .expect("failed to connect to database");
    db.migrate().await.expect("failed to run migrations");

    let user = User {
        id: UserId::new(),
        username: username.to_string(),
        password_hash: core_auth::hash_password(password).expect("failed to hash password"),
        // Shell access to run this command already implies operator-level
        // trust, so it always mints an admin — unlike web registration,
        // where only the very first account gets that automatically.
        is_admin: true,
        created_at: OffsetDateTime::now_utc(),
        totp_secret: None,
        totp_enabled: false,
        totp_recovery_codes: Vec::new(),
    };

    db.create_user(&user).await.expect("failed to create user");
    println!("created user '{username}'");
}
