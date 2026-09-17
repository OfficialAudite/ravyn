use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use ravyn_core::{auth as core_auth, ShortUrl, ShortUrlId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{auth::AuthedUser, state::AppState};

const SLUG_LENGTH: usize = 7;
const MAX_SLUG_ATTEMPTS: u8 = 5;

#[derive(Deserialize)]
pub struct CreateShortUrlRequest {
    destination: String,
}

/// Retries with a fresh random slug on a collision (`generate_slug`'s own
/// doc comment covers why this is the right place to handle that, not
/// generation itself) — at `SLUG_LENGTH` this should essentially never
/// happen, so `MAX_SLUG_ATTEMPTS` exists only as a hard stop against an
/// unlucky streak turning into an infinite loop.
pub async fn create_short_url(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Json(body): Json<CreateShortUrlRequest>,
) -> Response {
    let destination = body.destination.trim().to_string();
    if !(destination.starts_with("http://") || destination.starts_with("https://")) {
        return (
            StatusCode::BAD_REQUEST,
            "destination must start with http:// or https://",
        )
            .into_response();
    }

    for attempt in 0..MAX_SLUG_ATTEMPTS {
        let short_url = ShortUrl {
            id: ShortUrlId::new(),
            owner_id: user.id,
            slug: core_auth::generate_slug(SLUG_LENGTH),
            destination: destination.clone(),
            clicks: 0,
            created_at: OffsetDateTime::now_utc(),
        };

        match state.db.create_short_url(&short_url).await {
            Ok(()) => {
                return Json(serde_json::json!({
                    "id": short_url.id.0,
                    "slug": short_url.slug,
                }))
                .into_response()
            }
            Err(err) if err.is_unique_violation() && attempt + 1 < MAX_SLUG_ATTEMPTS => continue,
            Err(err) => {
                tracing::error!(%err, "failed to create short url");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }

    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

#[derive(Serialize)]
pub struct ShortUrlSummary {
    id: Uuid,
    slug: String,
    destination: String,
    clicks: i64,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

impl From<ShortUrl> for ShortUrlSummary {
    fn from(short_url: ShortUrl) -> Self {
        ShortUrlSummary {
            id: short_url.id.0,
            slug: short_url.slug,
            destination: short_url.destination,
            clicks: short_url.clicks,
            created_at: short_url.created_at,
        }
    }
}

pub async fn list_short_urls(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
) -> Response {
    let short_urls = match state.db.list_short_urls_for_owner(user.id).await {
        Ok(short_urls) => short_urls,
        Err(err) => {
            tracing::error!(%err, "failed to list short urls");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let summaries: Vec<ShortUrlSummary> =
        short_urls.into_iter().map(ShortUrlSummary::from).collect();
    Json(summaries).into_response()
}

pub async fn delete_short_url(
    AuthedUser(user): AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    let short_url = match state.db.get_short_url(ShortUrlId(id)).await {
        Ok(Some(short_url)) => short_url,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up short url");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if short_url.owner_id != user.id {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(err) = state.db.delete_short_url(short_url.id).await {
        tracing::error!(%err, "failed to delete short url");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// Public — this is the whole point of a short link. Best-effort click
/// count: if the increment fails, the redirect still happens, since a
/// click that doesn't count is a much smaller problem than a link that
/// doesn't work.
pub async fn redirect_short_url(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Response {
    let short_url = match state.db.get_short_url_by_slug(&slug).await {
        Ok(Some(short_url)) => short_url,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up short url");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Err(err) = state.db.increment_short_url_clicks(short_url.id).await {
        tracing::warn!(%err, "failed to record short url click");
    }

    Redirect::to(&short_url.destination).into_response()
}
