//! Run against a real Postgres: `DATABASE_URL=... cargo test -- --include-ignored`.
//! `sqlx::test` gives each test its own freshly migrated database.

use ravyn_core::{File, FileId, Invite, InviteId, Session, User, UserId};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::Db;

fn new_user(username: &str) -> User {
    User {
        id: UserId::new(),
        username: username.to_string(),
        password_hash: "x".to_string(),
        is_admin: false,
        created_at: OffsetDateTime::now_utc(),
        totp_secret: None,
        totp_enabled: false,
        totp_recovery_codes: Vec::new(),
    }
}

async fn user(db: &Db, username: &str) -> User {
    let user = new_user(username);
    db.create_user(&user).await.unwrap();
    user
}

async fn file(db: &Db, owner: UserId, content_type: &str, size_bytes: u64) {
    let file = File {
        id: FileId::new(),
        owner_id: owner,
        original_name: "f".to_string(),
        storage_key: Uuid::new_v4().to_string(),
        content_type: content_type.to_string(),
        size_bytes,
        sha256: String::new(),
        folder_id: None,
        password_hash: None,
        created_at: OffsetDateTime::now_utc(),
        expires_at: None,
        tags: Vec::new(),
    };
    db.insert_file(&file).await.unwrap();
}

async fn session(db: &Db, user_id: UserId, token_hash: &str, expires_in: Duration) {
    let now = OffsetDateTime::now_utc();
    db.create_session(&Session {
        token_hash: token_hash.to_string(),
        user_id,
        expires_at: now + expires_in,
        created_at: now,
    })
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL"]
async fn usernames_are_unique_ignoring_case(pool: PgPool) {
    let db = Db { pool };
    user(&db, "Alice").await;

    assert!(db.username_taken("alice").await.unwrap());
    assert!(db.username_taken("ALICE").await.unwrap());
    assert!(!db.username_taken("bob").await.unwrap());

    let err = db.create_user(&new_user("alice")).await.unwrap_err();
    assert!(err.is_unique_violation());
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL"]
async fn an_invite_can_only_be_claimed_once(pool: PgPool) {
    let db = Db { pool };
    let admin = user(&db, "admin").await;
    let invite = Invite {
        id: InviteId::new(),
        token_hash: "hash".to_string(),
        created_by: admin.id,
        created_at: OffsetDateTime::now_utc(),
        used_by: None,
        used_at: None,
    };
    db.create_invite(&invite).await.unwrap();

    let claims = claim_concurrently(&db, invite.id).await;
    assert_eq!(claims.iter().filter(|claimed| **claimed).count(), 1);
    assert!(db
        .get_unused_invite_by_token_hash("hash")
        .await
        .unwrap()
        .is_none());

    db.release_invite(invite.id).await.unwrap();
    assert!(db.claim_invite(invite.id).await.unwrap());
}

async fn claim_concurrently(db: &Db, id: InviteId) -> Vec<bool> {
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let db = db.clone();
            tokio::spawn(async move { db.claim_invite(id).await.unwrap() })
        })
        .collect();
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }
    results
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL"]
async fn a_recovery_code_works_once(pool: PgPool) {
    let db = Db { pool };
    let alice = user(&db, "alice").await;
    db.set_pending_totp_secret(alice.id, "secret".to_string())
        .await
        .unwrap();
    db.enable_totp(alice.id, vec!["code-a".to_string(), "code-b".to_string()])
        .await
        .unwrap();

    assert!(db.consume_recovery_code(alice.id, "code-a").await.unwrap());
    assert!(!db.consume_recovery_code(alice.id, "code-a").await.unwrap());
    assert!(!db
        .consume_recovery_code(alice.id, "never-issued")
        .await
        .unwrap());
    assert!(db.consume_recovery_code(alice.id, "code-b").await.unwrap());
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL"]
async fn sessions_are_revoked_and_swept(pool: PgPool) {
    let db = Db { pool };
    let alice = user(&db, "alice").await;
    let bob = user(&db, "bob").await;
    session(&db, alice.id, "alice-current", Duration::days(1)).await;
    session(&db, alice.id, "alice-stolen", Duration::days(1)).await;
    session(&db, bob.id, "bob", Duration::days(1)).await;
    session(&db, bob.id, "bob-expired", -Duration::days(1)).await;

    db.delete_other_sessions(alice.id, Some("alice-current"))
        .await
        .unwrap();
    db.delete_expired_sessions().await.unwrap();

    let remaining: Vec<String> =
        sqlx::query_scalar("select token_hash from sessions order by token_hash")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert_eq!(remaining, ["alice-current", "bob"]);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL"]
async fn stats_are_bucketed_per_owner_and_instance(pool: PgPool) {
    let db = Db { pool };
    let alice = user(&db, "alice").await;
    let bob = user(&db, "bob").await;
    file(&db, alice.id, "image/png", 100).await;
    file(&db, alice.id, "video/mp4", 1000).await;
    file(&db, alice.id, "text/plain", 10).await;
    file(&db, alice.id, "application/pdf", 20).await;
    file(&db, bob.id, "application/zip", 5).await;

    let mine = db.file_stats(Some(alice.id)).await.unwrap();
    assert_eq!((mine.file_count, mine.total_bytes), (4, 1130));
    assert_eq!((mine.images, mine.videos, mine.audio), (1, 1, 0));
    assert_eq!((mine.documents, mine.other), (2, 0));

    let all = db.file_stats(None).await.unwrap();
    assert_eq!((all.file_count, all.total_bytes, all.other), (5, 1135, 1));

    let usage = db.list_users_with_usage().await.unwrap();
    let summary: Vec<_> = usage
        .iter()
        .map(|u| (u.user.username.as_str(), u.file_count, u.storage_used_bytes))
        .collect();
    assert_eq!(summary, [("alice", 4, 1130), ("bob", 1, 5)]);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "needs DATABASE_URL"]
async fn upload_settings_match_the_individual_getters(pool: PgPool) {
    let db = Db { pool };
    db.set_strip_exif(false).await.unwrap();
    db.set_random_name_length(12).await.unwrap();

    let settings = db.get_upload_settings().await.unwrap();
    assert!(!settings.strip_exif);
    assert_eq!(settings.random_name_length, 12);
    assert_eq!(
        settings.naming_scheme.as_str(),
        db.get_naming_scheme().await.unwrap().as_str()
    );
}
