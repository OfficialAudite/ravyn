use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use ravyn_core::{EmbedSettings, File, FileId};
use serde::Deserialize;
use uuid::Uuid;

use super::{is_authorized, password_prompt_html, resolve_public_base_url};
use crate::{auth::AuthedUser, state::AppState};

#[derive(Deserialize)]
pub struct ViewQuery {
    password: Option<String>,
}

/// The stable "share" link — this is what copy-link, the shared folder
/// page, and the ShareX config all point at, instead of the raw
/// `/files/{id}` URL directly.
///
/// If the owner hasn't turned embeds on, this just redirects straight to
/// the raw file: Discord (and friends) follow redirects and preview the
/// raw image/video at the far end exactly like they always have, so
/// nothing changes for anyone who doesn't care about this feature. Only
/// when embeds are enabled does this render an actual HTML page carrying
/// Open Graph tags, since a raw image URL has nowhere to put a custom
/// title or description.
pub async fn view_file(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<ViewQuery>,
    requester: Option<AuthedUser>,
    headers: HeaderMap,
) -> Response {
    let file = match state.db.get_file(FileId(id)).await {
        Ok(Some(file)) => file,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::error!(%err, "failed to look up file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let requester = requester.map(|AuthedUser(user)| user);
    if !is_authorized(&file, requester.as_ref(), query.password.as_deref()) {
        let message = if query.password.is_some() {
            "wrong password"
        } else {
            "this file is password protected"
        };
        return (
            StatusCode::UNAUTHORIZED,
            Html(password_prompt_html(message)),
        )
            .into_response();
    }

    // A readable, in-page view for text pastes (pasted snippets, or any
    // plain-text upload) rather than either a redirect to a bare raw
    // response or, worse, the generic embed page's fallback
    // "<p>title</p><a>download</a>" — independent of the owner's embed
    // setting, since this is about making text legible, not social-preview
    // metadata.
    if is_text_content(&file.content_type) {
        let base = resolve_public_base_url(&state, &headers);
        return render_text_view(&state, &file, &base).await;
    }

    let embed = match state.db.get_embed_settings(file.owner_id).await {
        Ok(settings) => settings,
        Err(err) => {
            tracing::error!(%err, "failed to load embed settings");
            EmbedSettings::default()
        }
    };

    if !embed.enabled {
        return Redirect::to(&format!("/files/{id}")).into_response();
    }

    let owner_username = match state.db.get_user_by_id(file.owner_id).await {
        Ok(Some(user)) => user.username,
        _ => "someone".to_string(),
    };

    let base = resolve_public_base_url(&state, &headers);
    Html(render_view_html(&file, &embed, &owner_username, &base)).into_response()
}

fn is_text_content(content_type: &str) -> bool {
    content_type.starts_with("text/") || content_type == "application/json"
}

/// Above this, a text file falls back to the plain raw response instead of
/// being inlined into an HTML page — a pastebin-style preview makes sense
/// for a snippet, not for a multi-megabyte log dump.
const MAX_INLINE_TEXT_BYTES: u64 = 2 * 1024 * 1024;

async fn render_text_view(state: &AppState, file: &File, base: &str) -> Response {
    if file.size_bytes > MAX_INLINE_TEXT_BYTES {
        return Redirect::to(&format!("/files/{}", file.id.0)).into_response();
    }

    let bytes = match state.storage.get(&file.storage_key).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(%err, "failed to read file from storage");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let title = escape_html(&file.original_name);
    let raw_url = escape_html(&format!("{base}/files/{}", file.id.0));
    // Escaped before ever touching the HTML string — this is arbitrary
    // user-pasted content, so treating it as anything but plain text here
    // would be a stored XSS hole.
    let body = escape_html(&String::from_utf8_lossy(&bytes));

    Html(format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  :root {{ color-scheme: dark; }}
  body {{ margin:0; min-height:100vh; background:#0c0d12; color:#eeecf5;
         font-family: ui-sans-serif, system-ui, sans-serif; }}
  header {{ display:flex; align-items:center; justify-content:space-between; gap:1rem;
            padding:0.85rem 1.25rem; border-bottom:1px solid #282a35; position:sticky; top:0;
            background:#0c0d12; }}
  header span {{ font-size:0.85rem; color:#9291a3; overflow:hidden; text-overflow:ellipsis;
                 white-space:nowrap; }}
  header a {{ color:#5b8dff; text-decoration:none; font-size:0.85rem; flex-shrink:0; }}
  header a:hover {{ text-decoration:underline; }}
  pre {{ margin:0; padding:1.25rem; overflow-x:auto;
        font-family: ui-monospace, "SF Mono", Consolas, monospace;
        font-size:0.85rem; line-height:1.5; white-space:pre-wrap; word-break:break-word; }}
</style></head>
<body>
  <header>
    <span title="{title}">{title}</span>
    <a href="{raw_url}" download>download raw</a>
  </header>
  <pre>{body}</pre>
</body></html>"#
    ))
    .into_response()
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn apply_template(template: &str, file: &File, username: &str) -> String {
    template
        .replace("{file.name}", &file.original_name)
        .replace("{file.size}", &format_size(file.size_bytes))
        .replace("{file.type}", &file.content_type)
        .replace("{user.username}", username)
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    format!("{size:.1} {}", UNITS[unit])
}

fn render_view_html(file: &File, embed: &EmbedSettings, username: &str, base: &str) -> String {
    let raw_url = escape_html(&format!("{base}/files/{}", file.id.0));
    let page_url = escape_html(&format!("{base}/v/{}", file.id.0));

    let is_image = file.content_type.starts_with("image/");
    let is_video = file.content_type.starts_with("video/");
    let is_audio = file.content_type.starts_with("audio/");

    let title = escape_html(
        &embed
            .title
            .as_deref()
            .map(|t| apply_template(t, file, username))
            .unwrap_or_else(|| file.original_name.clone()),
    );
    let description = embed
        .description
        .as_deref()
        .map(|d| escape_html(&apply_template(d, file, username)));
    let site_name = escape_html(
        &embed
            .site_name
            .as_deref()
            .map(|s| apply_template(s, file, username))
            .unwrap_or_else(|| "ravyn".to_string()),
    );
    let color = escape_html(embed.color.as_deref().unwrap_or("#5b8dff"));

    let mut meta = vec![
        format!(r#"<meta property="og:title" content="{title}">"#),
        format!(r#"<meta property="og:site_name" content="{site_name}">"#),
        format!(r#"<meta name="theme-color" content="{color}">"#),
        format!(r#"<meta property="og:url" content="{page_url}">"#),
    ];
    if let Some(description) = &description {
        meta.push(format!(
            r#"<meta property="og:description" content="{description}">"#
        ));
    }

    let media_html = if is_image {
        meta.push(r#"<meta property="og:type" content="image">"#.to_string());
        meta.push(format!(r#"<meta property="og:image" content="{raw_url}">"#));
        meta.push(r#"<meta name="twitter:card" content="summary_large_image">"#.to_string());
        meta.push(format!(
            r#"<meta name="twitter:image" content="{raw_url}">"#
        ));
        format!(r#"<img src="{raw_url}" alt="{title}">"#)
    } else if is_video {
        let content_type = escape_html(&file.content_type);
        meta.push(r#"<meta property="og:type" content="video.other">"#.to_string());
        meta.push(format!(r#"<meta property="og:video" content="{raw_url}">"#));
        meta.push(format!(
            r#"<meta property="og:video:type" content="{content_type}">"#
        ));
        meta.push(r#"<meta property="og:video:width" content="1280">"#.to_string());
        meta.push(r#"<meta property="og:video:height" content="720">"#.to_string());
        format!(r#"<video src="{raw_url}" controls></video>"#)
    } else if is_audio {
        let content_type = escape_html(&file.content_type);
        meta.push(r#"<meta property="og:type" content="music.song">"#.to_string());
        meta.push(format!(r#"<meta property="og:audio" content="{raw_url}">"#));
        meta.push(format!(
            r#"<meta property="og:audio:type" content="{content_type}">"#
        ));
        meta.push(r#"<meta name="twitter:card" content="player">"#.to_string());
        meta.push(format!(
            r#"<meta name="twitter:player:stream" content="{raw_url}">"#
        ));
        format!(r#"<audio src="{raw_url}" controls></audio>"#)
    } else {
        format!(r#"<p>{title}</p><a href="{raw_url}">download</a>"#)
    };

    let meta = meta.join("\n");

    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
{meta}
<style>
  body {{ margin:0; min-height:100vh; display:grid; place-items:center; background:#0c0d12;
         color:#eeecf5; font-family: ui-sans-serif, system-ui, sans-serif; }}
  img, video {{ max-width:100%; max-height:90vh; }}
  a {{ color:#5b8dff; }}
</style></head>
<body>{media_html}</body></html>"#
    )
}
