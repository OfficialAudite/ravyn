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

    let storage_root = std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./data".into());

    let db = Db::connect(&database_url)
        .await
        .expect("failed to connect to database");
    db.migrate().await.expect("failed to run migrations");

    let storage = Storage::new(StorageConfig::Local { root: storage_root })
        .expect("failed to initialize storage");

    let state = AppState { db, storage };
    let app = routes::router(state);

    let addr: SocketAddr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()
        .expect("invalid LISTEN_ADDR");

    tracing::info!("listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
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
