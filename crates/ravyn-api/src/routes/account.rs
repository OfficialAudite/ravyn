use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use ravyn_core::{
    auth as core_auth, ApiToken, ApiTokenId, EmbedSettings, ExpiryPreset, Invite, InviteId,
    NamingScheme, RegistrationMode, User, UserId,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    auth::{AuthedUser, PENDING_LOGIN_LIFETIME, SESSION_COOKIE, SESSION_LIFETIME},
    rate_limit::client_ip,
    state::AppState,
};

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

/// For an account without 2FA, this signs straight in. For one with 2FA,
/// a correct password alone isn't enough — instead of a session, this
/// issues a short-lived `PendingLogin` and asks the client to follow up at
/// `POST /login/totp` with a code. The response shape tells the two apart:
/// `{"totp_required": true, "login_token": ...}` versus a plain session
/// cookie.
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    if !state.rate_limiters.login.check(&client_ip(&headers)) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts, try again later",
        )
            .into_response();
    }

    let user = match state.db.get_user_by_username(&body.username).await {
        Ok(Some(user)) => user,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up user");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if !core_auth::verify_password(&body.password, &user.password_hash) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if user.totp_enabled {
        return new_pending_login_response(&state, user.id).await;
    }

    let _ = state
        .db
        .log_activity(Some(user.id), &user.username, "logged in", None)
        .await;
    new_session_response(&state, user.id).await
}

async fn new_pending_login_response(state: &AppState, user_id: UserId) -> Response {
    let token = core_auth::generate_token();
    let pending = ravyn_core::PendingLogin {
        token_hash: core_auth::hash_token(&token),
        user_id,
        created_at: OffsetDateTime::now_utc(),
        expires_at: OffsetDateTime::now_utc() + PENDING_LOGIN_LIFETIME,
    };

    if let Err(err) = state.db.create_pending_login(&pending).await {
        tracing::error!(%err, "failed to create pending login");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(serde_json::json!({ "totp_required": true, "login_token": token })).into_response()
}

#[derive(Deserialize)]
pub struct TotpLoginRequest {
    login_token: String,
    code: String,
}

/// Completes a login started at `POST /login` for a 2FA account. Accepts
/// either a current TOTP code or an unused recovery code — either way, the
/// pending login is deleted here so it can't be redeemed twice, same as an
/// invite code.
pub async fn login_totp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<TotpLoginRequest>,
) -> Response {
    if !state.rate_limiters.totp.check(&client_ip(&headers)) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts, try again later",
        )
            .into_response();
    }

    let token_hash = core_auth::hash_token(&body.login_token);
    let pending = match state.db.get_pending_login(&token_hash).await {
        Ok(Some(pending)) => pending,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up pending login");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let user = match state.db.get_user_by_id(pending.user_id).await {
        Ok(Some(user)) => user,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up user");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let code = body.code.trim();
    let valid = user
        .totp_secret
        .as_deref()
        .is_some_and(|secret| core_auth::verify_totp(secret, code))
        || redeem_recovery_code(&state, &user, code).await;

    if !valid {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let _ = state.db.delete_pending_login(&token_hash).await;
    let _ = state
        .db
        .log_activity(Some(user.id), &user.username, "logged in", None)
        .await;
    new_session_response(&state, user.id).await
}

/// Tries `code` against each of the user's remaining recovery-code hashes;
/// on a match, consumes that one (single-use, like an invite code) and
/// reports success.
async fn redeem_recovery_code(state: &AppState, user: &User, code: &str) -> bool {
    let hash = core_auth::hash_token(code);
    if !user.totp_recovery_codes.contains(&hash) {
        return false;
    }
    let _ = state.db.consume_recovery_code(user.id, &hash).await;
    true
}

/// Creates a session for `user_id` and sets its cookie — shared by `login`
/// and `register`, since registering also signs you straight in.
async fn new_session_response(state: &AppState, user_id: UserId) -> Response {
    let token = core_auth::generate_token();
    let session = ravyn_core::Session {
        token_hash: core_auth::hash_token(&token),
        user_id,
        expires_at: OffsetDateTime::now_utc() + SESSION_LIFETIME,
        created_at: OffsetDateTime::now_utc(),
    };

    if let Err(err) = state.db.create_session(&session).await {
        tracing::error!(%err, "failed to create session");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // `.secure(true)` should be set once this sits behind a TLS-terminating
    // reverse proxy; left off here so local `http://` development still works.
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(SESSION_LIFETIME)
        .build();

    (CookieJar::new().add(cookie), StatusCode::OK).into_response()
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    username: String,
    password: String,
    invite_token: Option<String>,
}

/// The very first account on an instance always gets through here and
/// becomes an admin — there'd be no admin able to open registration up
/// otherwise. Every account after that is gated by whatever registration
/// mode the admin has set (`RegistrationMode`), checked fresh on every
/// call rather than cached anywhere.
pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> Response {
    if !state.rate_limiters.register.check(&client_ip(&headers)) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many registration attempts, try again later",
        )
            .into_response();
    }

    if body.username.trim().is_empty() || body.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "username and password are required",
        )
            .into_response();
    }

    let is_first_user = match state.db.has_any_users().await {
        Ok(any) => !any,
        Err(err) => {
            tracing::error!(%err, "failed to check for existing users");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut redeemed_invite = None;

    if !is_first_user {
        let mode = match state.db.get_registration_mode().await {
            Ok(mode) => mode,
            Err(err) => {
                tracing::error!(%err, "failed to load registration mode");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

        match mode {
            RegistrationMode::Closed => return StatusCode::FORBIDDEN.into_response(),
            RegistrationMode::Open => {}
            RegistrationMode::Invite => {
                let Some(token) = body.invite_token.as_deref().filter(|t| !t.is_empty()) else {
                    return (StatusCode::BAD_REQUEST, "an invite code is required").into_response();
                };

                let invite = match state
                    .db
                    .get_unused_invite_by_token_hash(&core_auth::hash_token(token))
                    .await
                {
                    Ok(Some(invite)) => invite,
                    Ok(None) => {
                        return (StatusCode::FORBIDDEN, "invalid or already-used invite code")
                            .into_response()
                    }
                    Err(err) => {
                        tracing::error!(%err, "failed to look up invite");
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                };
                redeemed_invite = Some(invite.id);
            }
        }
    }

    match state.db.get_user_by_username(&body.username).await {
        Ok(Some(_)) => return (StatusCode::CONFLICT, "username already taken").into_response(),
        Ok(None) => {}
        Err(err) => {
            tracing::error!(%err, "failed to look up user");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    let password_hash = match core_auth::hash_password(&body.password) {
        Ok(hash) => hash,
        Err(err) => {
            tracing::error!(%err, "failed to hash password");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let user = User {
        id: UserId::new(),
        username: body.username,
        password_hash,
        is_admin: is_first_user,
        created_at: OffsetDateTime::now_utc(),
        totp_secret: None,
        totp_enabled: false,
        totp_recovery_codes: Vec::new(),
    };

    if let Err(err) = state.db.create_user(&user).await {
        tracing::error!(%err, "failed to create user");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if let Some(invite_id) = redeemed_invite {
        if let Err(err) = state.db.mark_invite_used(invite_id, user.id).await {
            tracing::warn!(%err, "failed to mark invite as used");
        }
    }

    let action = if is_first_user {
        "registered (first admin)"
    } else {
        "registered"
    };
    let _ = state
        .db
        .log_activity(Some(user.id), &user.username, action, None)
        .await;

    new_session_response(&state, user.id).await
}

/// Public: lets the login screen decide whether to offer "create an
/// account" (and what it needs to ask for) without requiring auth to find
/// out — there's nothing sensitive in "is this instance claimed yet" or
/// "what's the current registration mode".
pub async fn registration_status(State(state): State<AppState>) -> Response {
    let setup_required = match state.db.has_any_users().await {
        Ok(any) => !any,
        Err(err) => {
            tracing::error!(%err, "failed to check for existing users");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mode = match state.db.get_registration_mode().await {
        Ok(mode) => mode,
        Err(err) => {
            tracing::error!(%err, "failed to load registration mode");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(serde_json::json!({ "setup_required": setup_required, "mode": mode.as_str() }))
        .into_response()
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        let token_hash = core_auth::hash_token(cookie.value());
        let _ = state.db.delete_session(&token_hash).await;
    }

    (jar.remove(Cookie::from(SESSION_COOKIE)), StatusCode::OK).into_response()
}

pub async fn me(AuthedUser(user): AuthedUser) -> Response {
    Json(serde_json::json!({
        "username": user.username,
        "is_admin": user.is_admin,
        "totp_enabled": user.totp_enabled,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// Requires the current password rather than trusting the session alone —
/// the same reasoning most services have for this, since a session cookie
/// can outlive the moment someone meant to be signed in (a shared machine,
/// a stolen cookie) in a way a freshly-typed password can't.
pub async fn change_password(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<ChangePasswordRequest>,
) -> Response {
    if !core_auth::verify_password(&body.current_password, &user.password_hash) {
        return (StatusCode::UNAUTHORIZED, "current password is incorrect").into_response();
    }

    if body.new_password.is_empty() {
        return (StatusCode::BAD_REQUEST, "new password is required").into_response();
    }

    let password_hash = match core_auth::hash_password(&body.new_password) {
        Ok(hash) => hash,
        Err(err) => {
            tracing::error!(%err, "failed to hash password");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Err(err) = state.db.set_password_hash(user.id, password_hash).await {
        tracing::error!(%err, "failed to save new password");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let _ = state
        .db
        .log_activity(Some(user.id), &user.username, "changed password", None)
        .await;

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Serialize)]
pub struct TotpSetupResponse {
    /// Shown alongside the QR code for manual entry — some authenticator
    /// apps or setups (a desktop-only password manager, a headless
    /// terminal) can't scan one.
    secret: String,
    otpauth_url: String,
    /// A base64-encoded PNG, embeddable directly as `data:image/png;base64,...`.
    qr_code_base64: String,
}

/// Starts 2FA setup: generates a new secret and stores it as *pending* —
/// `totp_enabled` stays false until `confirm_totp` proves the user's
/// authenticator app actually agrees with it. Refuses to run again while
/// 2FA is already on, so a stolen session alone can't silently swap out an
/// account's second factor — that requires going through `disable_totp`
/// first, which needs the password too.
pub async fn setup_totp(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    if user.totp_enabled {
        return (
            StatusCode::BAD_REQUEST,
            "2FA is already enabled — disable it first",
        )
            .into_response();
    }

    let secret = core_auth::generate_totp_secret();
    let Some((otpauth_url, qr_code_base64)) =
        core_auth::totp_setup_uri(&secret, &user.username, "ravyn")
    else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    if let Err(err) = state
        .db
        .set_pending_totp_secret(user.id, secret.clone())
        .await
    {
        tracing::error!(%err, "failed to save pending totp secret");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(TotpSetupResponse {
        secret,
        otpauth_url,
        qr_code_base64,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct ConfirmTotpRequest {
    code: String,
}

/// Confirms the pending secret from `setup_totp` with a real code from the
/// user's authenticator app, turns 2FA on, and mints a fresh set of
/// recovery codes — shown here, once, same as an API token.
pub async fn confirm_totp(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<ConfirmTotpRequest>,
) -> Response {
    let Some(secret) = user.totp_secret.as_deref() else {
        return (StatusCode::BAD_REQUEST, "start 2FA setup first").into_response();
    };

    if !core_auth::verify_totp(secret, body.code.trim()) {
        return (StatusCode::UNAUTHORIZED, "invalid code").into_response();
    }

    let recovery_codes = core_auth::generate_recovery_codes(8);
    let hashes = recovery_codes
        .iter()
        .map(|code| core_auth::hash_token(code))
        .collect();

    if let Err(err) = state.db.enable_totp(user.id, hashes).await {
        tracing::error!(%err, "failed to enable totp");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let _ = state
        .db
        .log_activity(Some(user.id), &user.username, "enabled 2FA", None)
        .await;

    Json(serde_json::json!({ "recovery_codes": recovery_codes })).into_response()
}

#[derive(Deserialize)]
pub struct DisableTotpRequest {
    password: String,
    code: String,
}

/// Requires both the password and a valid code (TOTP or recovery) — a
/// stolen session cookie alone can't turn off someone's second factor.
pub async fn disable_totp(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<DisableTotpRequest>,
) -> Response {
    if !core_auth::verify_password(&body.password, &user.password_hash) {
        return (StatusCode::UNAUTHORIZED, "current password is incorrect").into_response();
    }

    let code = body.code.trim();
    let code_ok = user
        .totp_secret
        .as_deref()
        .is_some_and(|secret| core_auth::verify_totp(secret, code))
        || redeem_recovery_code(&state, &user, code).await;

    if !code_ok {
        return (StatusCode::UNAUTHORIZED, "invalid code").into_response();
    }

    if let Err(err) = state.db.disable_totp(user.id).await {
        tracing::error!(%err, "failed to disable totp");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let _ = state
        .db
        .log_activity(Some(user.id), &user.username, "disabled 2FA", None)
        .await;

    StatusCode::NO_CONTENT.into_response()
}

/// Every field optional so the registration form and the naming-scheme form
/// (two independent forms on the same admin page) can each save just their
/// own settings without clobbering the other's.
#[derive(Deserialize)]
pub struct SetInstanceSettingsRequest {
    registration_mode: Option<String>,
    naming_scheme: Option<String>,
    random_name_length: Option<i64>,
    default_expiry_preset: Option<String>,
    strip_exif: Option<bool>,
}

pub async fn get_instance_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mode = match state.db.get_registration_mode().await {
        Ok(mode) => mode,
        Err(err) => {
            tracing::error!(%err, "failed to load registration mode");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let naming_scheme = match state.db.get_naming_scheme().await {
        Ok(scheme) => scheme,
        Err(err) => {
            tracing::error!(%err, "failed to load naming scheme");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let random_name_length = state.db.get_random_name_length().await.unwrap_or(8);
    let default_expiry_preset = state
        .db
        .get_default_expiry_preset()
        .await
        .unwrap_or(ExpiryPreset::Never);
    let strip_exif = state.db.get_strip_exif().await.unwrap_or(true);

    Json(serde_json::json!({
        "registration_mode": mode.as_str(),
        "naming_scheme": naming_scheme.as_str(),
        "random_name_length": random_name_length,
        "default_expiry_preset": default_expiry_preset.as_str(),
        "strip_exif": strip_exif,
    }))
    .into_response()
}

/// Clamped rather than rejected: a stray very-short or very-long value from
/// a hand-edited request is a nuisance, not something worth failing an
/// otherwise-valid save over.
const MIN_RANDOM_NAME_LENGTH: i64 = 4;
const MAX_RANDOM_NAME_LENGTH: i64 = 64;

pub async fn set_instance_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<SetInstanceSettingsRequest>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Some(raw_mode) = body.registration_mode {
        let Some(mode) = RegistrationMode::parse(&raw_mode) else {
            return (StatusCode::BAD_REQUEST, "invalid registration mode").into_response();
        };
        if let Err(err) = state.db.set_registration_mode(mode).await {
            tracing::error!(%err, "failed to save registration mode");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Some(raw_scheme) = body.naming_scheme {
        let Some(scheme) = NamingScheme::parse(&raw_scheme) else {
            return (StatusCode::BAD_REQUEST, "invalid naming scheme").into_response();
        };
        if let Err(err) = state.db.set_naming_scheme(scheme).await {
            tracing::error!(%err, "failed to save naming scheme");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Some(length) = body.random_name_length {
        let length = length.clamp(MIN_RANDOM_NAME_LENGTH, MAX_RANDOM_NAME_LENGTH);
        if let Err(err) = state.db.set_random_name_length(length).await {
            tracing::error!(%err, "failed to save random name length");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Some(raw_preset) = body.default_expiry_preset {
        let Some(preset) = ExpiryPreset::parse(&raw_preset) else {
            return (StatusCode::BAD_REQUEST, "invalid expiry preset").into_response();
        };
        if let Err(err) = state.db.set_default_expiry_preset(preset).await {
            tracing::error!(%err, "failed to save default expiry preset");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Some(strip_exif) = body.strip_exif {
        if let Err(err) = state.db.set_strip_exif(strip_exif).await {
            tracing::error!(%err, "failed to save strip-exif setting");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    let _ = state
        .db
        .log_activity(
            Some(user.id),
            &user.username,
            "changed instance settings",
            None,
        )
        .await;

    StatusCode::NO_CONTENT.into_response()
}

/// Mints a single-use invite code for invite-only registration. The
/// plaintext code is only ever returned here, once — same pattern as an API
/// token.
pub async fn create_invite(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let token = core_auth::generate_token();
    let invite = Invite {
        id: InviteId::new(),
        token_hash: core_auth::hash_token(&token),
        created_by: user.id,
        created_at: OffsetDateTime::now_utc(),
        used_by: None,
        used_at: None,
    };

    if let Err(err) = state.db.create_invite(&invite).await {
        tracing::error!(%err, "failed to create invite");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(serde_json::json!({ "id": invite.id.0, "token": token })).into_response()
}

#[derive(Serialize)]
pub struct InviteSummary {
    id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    used: bool,
}

impl From<Invite> for InviteSummary {
    fn from(invite: Invite) -> Self {
        InviteSummary {
            id: invite.id.0,
            created_at: invite.created_at,
            used: invite.used_at.is_some(),
        }
    }
}

pub async fn list_invites(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    match state.db.list_invites().await {
        Ok(invites) => {
            let summaries: Vec<InviteSummary> =
                invites.into_iter().map(InviteSummary::from).collect();
            Json(summaries).into_response()
        }
        Err(err) => {
            tracing::error!(%err, "failed to list invites");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn delete_invite(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(err) = state.db.delete_invite(InviteId(id)).await {
        tracing::error!(%err, "failed to delete invite");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Serialize)]
pub struct AdminUserSummary {
    id: Uuid,
    username: String,
    is_admin: bool,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    storage_used_bytes: i64,
    max_storage_bytes: Option<i64>,
    file_count: i64,
}

/// Every account on the instance with its current storage usage — an
/// admin's view of who's using what, and the basis for setting limits.
/// Usage is computed per user rather than fetched in one join here; at the
/// scale a self-hosted instance runs at, N+1 simple indexed queries costs
/// nothing and keeps this handler from having to know how that join works.
pub async fn list_users(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let users = match state.db.list_users().await {
        Ok(users) => users,
        Err(err) => {
            tracing::error!(%err, "failed to list users");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut summaries = Vec::with_capacity(users.len());
    for u in users {
        let storage_used_bytes = state.db.get_storage_usage(u.id).await.unwrap_or(0);
        let max_storage_bytes = state.db.get_max_storage_bytes(u.id).await.unwrap_or(None);
        let file_count = state.db.count_files_for_owner(u.id).await.unwrap_or(0);
        summaries.push(AdminUserSummary {
            id: u.id.0,
            username: u.username,
            is_admin: u.is_admin,
            created_at: u.created_at,
            storage_used_bytes,
            max_storage_bytes,
            file_count,
        });
    }

    Json(summaries).into_response()
}

#[derive(Deserialize)]
pub struct SetUserLimitRequest {
    max_storage_bytes: Option<i64>,
}

pub async fn set_user_limit(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetUserLimitRequest>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(err) = state
        .db
        .set_max_storage_bytes(UserId(id), body.max_storage_bytes)
        .await
    {
        tracing::error!(%err, "failed to set storage limit");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let target_username = state
        .db
        .get_user_by_id(UserId(id))
        .await
        .ok()
        .flatten()
        .map(|target| target.username);
    let limit_description = match body.max_storage_bytes {
        Some(bytes) => format!("{bytes} bytes"),
        None => "unlimited".to_string(),
    };
    let _ = state
        .db
        .log_activity(
            Some(user.id),
            &user.username,
            "set a storage limit",
            target_username
                .map(|name| format!("{name} -> {limit_description}"))
                .as_deref(),
        )
        .await;

    StatusCode::NO_CONTENT.into_response()
}

/// Deletes an account and everything it owns: every foreign key pointing
/// at `users.id` cascades in the database (files, folders, sessions, api
/// tokens, short urls, chunked uploads), but that only drops rows - the
/// actual bytes in storage need deleting first, same as a single file
/// delete already has to do. Refuses to delete the caller's own account,
/// since that's how an instance ends up with no admin able to fix it.
pub async fn delete_user(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    if id == user.id.0 {
        return (StatusCode::BAD_REQUEST, "can't delete your own account").into_response();
    }

    let target = match state.db.get_user_by_id(UserId(id)).await {
        Ok(Some(target)) => target,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up user");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let files = state
        .db
        .list_files_for_owner(target.id)
        .await
        .unwrap_or_default();
    for file in &files {
        let _ = state.storage.delete(&file.storage_key).await;
        let _ = state
            .thumbnails
            .delete(&super::files::thumbnail_key(file.id))
            .await;
    }

    let chunked_uploads = state
        .db
        .list_chunked_uploads_for_owner(target.id)
        .await
        .unwrap_or_default();
    for upload in &chunked_uploads {
        if let Ok(parts) = state.db.list_chunked_upload_parts(upload.id).await {
            for part in parts {
                let _ = state.storage.delete(&part.storage_key).await;
            }
        }
    }

    if let Err(err) = state.db.delete_user(target.id).await {
        tracing::error!(%err, "failed to delete user");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let _ = state
        .db
        .log_activity(
            Some(user.id),
            &user.username,
            "deleted a user",
            Some(&target.username),
        )
        .await;

    StatusCode::NO_CONTENT.into_response()
}

/// Buckets a content type the same way the browse page's type filter does
/// (`TypeFilter` in `crates/ravyn-web/src/dashboard.rs`) — kept as a small
/// duplicate here rather than a shared crate, since it's four lines and the
/// two call sites (a filter predicate, a stats tally) want it in different
/// shapes anyway.
#[derive(Default, Serialize)]
struct TypeCounts {
    images: i64,
    videos: i64,
    audio: i64,
    documents: i64,
    other: i64,
}

impl TypeCounts {
    fn add(&mut self, content_type: &str) {
        if content_type.starts_with("image/") {
            self.images += 1;
        } else if content_type.starts_with("video/") {
            self.videos += 1;
        } else if content_type.starts_with("audio/") {
            self.audio += 1;
        } else if content_type == "application/pdf" || content_type.starts_with("text/") {
            self.documents += 1;
        } else {
            self.other += 1;
        }
    }
}

#[derive(Serialize)]
pub struct MyStats {
    file_count: i64,
    storage_used_bytes: i64,
    max_storage_bytes: Option<i64>,
    #[serde(flatten)]
    by_type: TypeCounts,
}

/// Every user's own view of their usage — unlike `list_users`, open to
/// anyone, not just an admin.
pub async fn my_stats(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    let files = match state.db.list_files_for_owner(user.id).await {
        Ok(files) => files,
        Err(err) => {
            tracing::error!(%err, "failed to list files for stats");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let max_storage_bytes = state
        .db
        .get_max_storage_bytes(user.id)
        .await
        .unwrap_or(None);

    let mut by_type = TypeCounts::default();
    let mut storage_used_bytes = 0i64;
    for file in &files {
        storage_used_bytes += file.size_bytes as i64;
        by_type.add(&file.content_type);
    }

    Json(MyStats {
        file_count: files.len() as i64,
        storage_used_bytes,
        max_storage_bytes,
        by_type,
    })
    .into_response()
}

#[derive(Serialize)]
pub struct InstanceStats {
    total_users: i64,
    total_files: i64,
    total_storage_bytes: i64,
    #[serde(flatten)]
    by_type: TypeCounts,
    /// `None` until an admin fills in a price on the cost estimate form -
    /// there's nothing sensible to guess at otherwise.
    estimated_monthly_cost: Option<f64>,
    cost_currency: String,
}

/// A gibibyte, matching the base the UI already uses everywhere else it
/// shows a size (`format_size` in `crates/ravyn-web/src/format.rs`) - the
/// estimate should agree with the number sitting right next to it.
const BYTES_PER_GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// The instance-wide counterpart to `my_stats` — every user's usage summed
/// together, admin only. Reuses `list_files_for_owner` per user rather than
/// a new "every file regardless of owner" query, for the same reason
/// `list_users` already does N+1 queries for the per-user list: at
/// self-hosted scale it's simpler than it is slow.
pub async fn admin_stats(AuthedUser(user): AuthedUser, State(state): State<AppState>) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let users = match state.db.list_users().await {
        Ok(users) => users,
        Err(err) => {
            tracing::error!(%err, "failed to list users for stats");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut by_type = TypeCounts::default();
    let mut total_files = 0i64;
    let mut total_storage_bytes = 0i64;

    for u in &users {
        let files = state
            .db
            .list_files_for_owner(u.id)
            .await
            .unwrap_or_default();
        total_files += files.len() as i64;
        for file in &files {
            total_storage_bytes += file.size_bytes as i64;
            by_type.add(&file.content_type);
        }
    }

    let (cost_per_gb_month, cost_currency) = state
        .db
        .get_cost_settings()
        .await
        .unwrap_or((None, "$".to_string()));
    let estimated_monthly_cost =
        cost_per_gb_month.map(|price| (total_storage_bytes as f64 / BYTES_PER_GB) * price);

    Json(InstanceStats {
        total_users: users.len() as i64,
        total_files,
        total_storage_bytes,
        by_type,
        estimated_monthly_cost,
        cost_currency,
    })
    .into_response()
}

#[derive(Serialize)]
pub struct CostSettingsResponse {
    cost_per_gb_month: Option<f64>,
    cost_currency: String,
}

/// Read side of the cost estimate form: whatever price-per-GB an admin has
/// filled in, purely for display and editing - `admin_stats` is what
/// actually multiplies it against real usage.
pub async fn get_cost_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    match state.db.get_cost_settings().await {
        Ok((cost_per_gb_month, cost_currency)) => Json(CostSettingsResponse {
            cost_per_gb_month,
            cost_currency,
        })
        .into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to load cost settings");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct SetCostSettingsRequest {
    cost_per_gb_month: Option<f64>,
    cost_currency: String,
}

/// A single dedicated form rather than folding this into
/// `set_instance_settings`'s "every field optional" shape: an admin
/// clearing the price back to "not configured" needs to send an explicit
/// `null`, which that endpoint's convention (an absent field means "leave
/// this alone") can't express.
pub async fn set_cost_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<SetCostSettingsRequest>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let currency = body.cost_currency.trim();
    let currency = if currency.is_empty() { "$" } else { currency };

    if let Err(err) = state
        .db
        .set_cost_settings(body.cost_per_gb_month, currency)
        .await
    {
        tracing::error!(%err, "failed to save cost settings");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
pub struct CreateApiTokenRequest {
    name: String,
}

/// Mints a new API token for the authenticated user, e.g. to paste into a
/// ShareX custom uploader config. The plaintext token is only ever returned
/// here, once.
pub async fn create_api_token(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<CreateApiTokenRequest>,
) -> Response {
    let token = core_auth::generate_token();
    let api_token = ApiToken {
        id: ApiTokenId::new(),
        token_hash: core_auth::hash_token(&token),
        user_id: user.id,
        name: body.name,
        created_at: OffsetDateTime::now_utc(),
        last_used_at: None,
    };

    if let Err(err) = state.db.create_api_token(&api_token).await {
        tracing::error!(%err, "failed to create api token");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(serde_json::json!({ "id": api_token.id.0, "token": token })).into_response()
}

#[derive(Serialize)]
pub struct ApiTokenSummary {
    id: Uuid,
    name: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    last_used_at: Option<OffsetDateTime>,
}

impl From<ApiToken> for ApiTokenSummary {
    fn from(token: ApiToken) -> Self {
        ApiTokenSummary {
            id: token.id.0,
            name: token.name,
            created_at: token.created_at,
            last_used_at: token.last_used_at,
        }
    }
}

pub async fn list_api_tokens(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    let tokens = match state.db.list_api_tokens_for_user(user.id).await {
        Ok(tokens) => tokens,
        Err(err) => {
            tracing::error!(%err, "failed to list api tokens");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let summaries: Vec<ApiTokenSummary> = tokens.into_iter().map(ApiTokenSummary::from).collect();
    Json(summaries).into_response()
}

pub async fn delete_api_token(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    let token = match state.db.get_api_token(ApiTokenId(id)).await {
        Ok(Some(token)) => token,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up api token");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if token.user_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(err) = state.db.delete_api_token(token.id).await {
        tracing::error!(%err, "failed to delete api token");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn get_embed_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    match state.db.get_embed_settings(user.id).await {
        Ok(settings) => Json(settings).into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to load embed settings");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn set_embed_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<EmbedSettings>,
) -> Response {
    if let Err(err) = state.db.set_embed_settings(user.id, &body).await {
        tracing::error!(%err, "failed to save embed settings");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn get_webhook_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    match state.db.get_webhook_url(user.id).await {
        Ok(webhook_url) => Json(serde_json::json!({ "webhook_url": webhook_url })).into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to load webhook url");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct SetWebhookSettingsRequest {
    webhook_url: Option<String>,
}

/// Posts a message to this URL every time this user uploads a file — see
/// `routes::files::notify_upload_webhook`. Deliberately no test-ping here:
/// saving already validates the shape (must look like a URL), and the next
/// real upload proves the rest.
pub async fn set_webhook_settings(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<SetWebhookSettingsRequest>,
) -> Response {
    let webhook_url = body.webhook_url.filter(|url| !url.trim().is_empty());
    if let Some(url) = &webhook_url {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return (
                StatusCode::BAD_REQUEST,
                "webhook url must start with http:// or https://",
            )
                .into_response();
        }
    }

    if let Err(err) = state.db.set_webhook_url(user.id, webhook_url).await {
        tracing::error!(%err, "failed to save webhook url");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Read-only: which storage backend is currently configured, for display on
/// the admin page. Never exposes credentials. Actually changing backends
/// still means editing environment variables and restarting, since that
/// config isn't stored in the database.
pub async fn storage_info(AuthedUser(user): AuthedUser) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    let backend = std::env::var("STORAGE_BACKEND").unwrap_or_default();
    if backend == "s3" {
        Json(serde_json::json!({
            "backend": "s3",
            "bucket": std::env::var("S3_BUCKET").ok(),
            "endpoint": std::env::var("S3_ENDPOINT").ok(),
        }))
        .into_response()
    } else {
        Json(serde_json::json!({
            "backend": "local",
            "root": std::env::var("STORAGE_ROOT").unwrap_or_else(|_| "./data".into()),
        }))
        .into_response()
    }
}

#[derive(Serialize)]
pub struct ActivityLogEntrySummary {
    id: Uuid,
    username: String,
    action: String,
    target: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

impl From<ravyn_core::ActivityLogEntry> for ActivityLogEntrySummary {
    fn from(entry: ravyn_core::ActivityLogEntry) -> Self {
        ActivityLogEntrySummary {
            id: entry.id,
            username: entry.username,
            action: entry.action,
            target: entry.target,
            created_at: entry.created_at,
        }
    }
}

/// The most recent activity across the whole instance, regardless of
/// owner - who uploaded or deleted what, who logged in, and what an
/// admin changed. `username` on each entry is a snapshot taken when the
/// action happened, not a live lookup, so this stays readable even for an
/// account since renamed or removed.
pub async fn list_activity(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    if !user.is_admin {
        return StatusCode::FORBIDDEN.into_response();
    }

    match state.db.list_recent_activity(200).await {
        Ok(entries) => {
            let summaries: Vec<ActivityLogEntrySummary> = entries
                .into_iter()
                .map(ActivityLogEntrySummary::from)
                .collect();
            Json(summaries).into_response()
        }
        Err(err) => {
            tracing::error!(%err, "failed to list activity log");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
