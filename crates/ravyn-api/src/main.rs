mod auth;
mod routes;
mod state;

use std::net::SocketAddr;

use ravyn_core::{auth as core_auth, User, UserId};
use ravyn_db::Db;
use ravyn_storage::{Storage, StorageConfig};
use state::AppState;
use time::OffsetDateTime;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

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
    };
    let app = routes::router(state);

    let addr: SocketAddr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()
        .expect("invalid LISTEN_ADDR");

    tracing::info!("listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
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

/// Bootstraps the first (or an additional) user. There is no self-service
/// registration by design — accounts are provisioned by whoever runs the
/// server: `cargo run -p ravyn-api -- create-user <username> <password>`.
async fn create_user(database_url: &str, username: &str, password: &str) {
    let db = Db::connect(database_url)
        .await
        .expect("failed to connect to database");
    db.migrate().await.expect("failed to run migrations");

    let user = User {
        id: UserId::new(),
        username: username.to_string(),
        password_hash: core_auth::hash_password(password).expect("failed to hash password"),
        created_at: OffsetDateTime::now_utc(),
    };

    db.create_user(&user).await.expect("failed to create user");
    println!("created user '{username}'");
}
