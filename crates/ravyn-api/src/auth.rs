use axum::{
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::{request::Parts, StatusCode},
};
use axum_extra::extract::CookieJar;
use ravyn_core::{auth, User};

use crate::state::AppState;

pub const SESSION_COOKIE: &str = "ravyn_session";
pub const SESSION_LIFETIME: time::Duration = time::Duration::days(30);

/// A request authenticated either by a session cookie (browser) or an
/// `Authorization: Bearer <token>` header (ShareX and other API clients).
pub struct AuthedUser(pub User);

impl FromRequestParts<AppState> for AuthedUser {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match resolve(parts, state).await? {
            Some(user) => Ok(AuthedUser(user)),
            None => Err(StatusCode::UNAUTHORIZED),
        }
    }
}

/// Lets a handler take `Option<AuthedUser>` for routes that are public but
/// behave differently for a logged-in owner (e.g. skipping a password
/// check on your own file).
impl OptionalFromRequestParts<AppState> for AuthedUser {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(resolve(parts, state).await?.map(AuthedUser))
    }
}

async fn resolve(parts: &mut Parts, state: &AppState) -> Result<Option<User>, StatusCode> {
    if let Some(token) = bearer_token(parts) {
        let token_hash = auth::hash_token(token);
        if let Some(user) = state
            .db
            .get_user_by_api_token_hash(&token_hash)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            let _ = state.db.touch_api_token(&token_hash).await;
            return Ok(Some(user));
        }
    }

    let jar = CookieJar::from_headers(&parts.headers);
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        let token_hash = auth::hash_token(cookie.value());
        if let Some(user) = state
            .db
            .get_user_by_session_token_hash(&token_hash)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            return Ok(Some(user));
        }
    }

    Ok(None)
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
