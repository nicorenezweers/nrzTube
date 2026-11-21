//! API für den Social-Bereich (TikTok, Instagram, X): Feed, Profile, Beiträge, Videos aus dem
//! Cache, gefolgte Konten und die Anmeldungen (cookies.txt) der Plattformen.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use nrztube_types::*;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::auth::{Admin, User};
use crate::routes::{bad, ApiError, R};
use crate::social::{clean_handle, valid_id, valid_platform};
use crate::AppState;

fn platform_ok(p: &str) -> Result<String, ApiError> {
    if valid_platform(p) { Ok(p.to_string()) } else { Err(bad("Unbekannte Plattform")) }
}

fn handle_ok(p: &str, h: &str) -> Result<String, ApiError> {
    clean_handle(p, h).ok_or_else(|| bad("Ungültiger Name"))
}

#[derive(Deserialize, Default)]
pub struct FeedQuery {
    #[serde(default)]
    platform: String,
    #[serde(default)]
    fresh: bool,
}

pub async fn feed(State(state): State<AppState>, User(me): User, Query(q): Query<FeedQuery>) -> Json<SocialFeed> {
    let settings = state.db.settings(me.id);
    let follows: Vec<SocialFollow> = state
        .db
        .social_follows(me.id)
        .into_iter()
        .filter(|f| settings.social_on(&f.platform))
        .filter(|f| q.platform.is_empty() || f.platform == q.platform)
        .collect();
    Json(state.social.feed(follows, q.fresh).await)
}

pub async fn profile(State(state): State<AppState>, User(_me): User, Path((p, h)): Path<(String, String)>, Query(q): Query<FeedQuery>) -> R<SocialProfile> {
    let p = platform_ok(&p)?;
    let h = handle_ok(&p, &h)?;
    let prof = state.social.profile(&p, &h, q.fresh).await?;
    Ok(Json((*prof).clone()))
}

#[derive(Deserialize, Default)]
pub struct PostQuery {
    #[serde(default)]
    h: String,
}

pub async fn post(State(state): State<AppState>, User(_me): User, Path((p, id)): Path<(String, String)>, Query(q): Query<PostQuery>) -> R<SocialPost> {
    let p = platform_ok(&p)?;
    if !valid_id(&p, &id) {
        return Err(bad("Ungültiger Beitrag"));
    }
    let h = clean_handle(&p, &q.h);
    let post = state.social.post(&p, &id, h.as_deref()).await?;
    Ok(Json((*post).clone()))
}

/// Video eines Beitrags: einmal herunterladen, dann mit Range-Unterstützung ausliefern
pub async fn play(State(state): State<AppState>, User(_me): User, Path((p, id)): Path<(String, String)>, Query(q): Query<PostQuery>, headers: HeaderMap) -> Response {
    if !valid_platform(&p) || !valid_id(&p, &id) {
        return bad("Ungültiger Beitrag").into_response();
    }
    let h = clean_handle(&p, &q.h);
    match state.social.video_file(&p, &id, h.as_deref()).await {
        Ok(path) => serve_file(&path, &headers).await,
        Err(e) => ApiError(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

/// Datei mit HTTP-Range ausliefern (Videos sind klein; höchstens 8 MB pro Antwort)
async fn serve_file(path: &std::path::Path, headers: &HeaderMap) -> Response {
    const CHUNK: u64 = 8 * 1024 * 1024;
    let Ok(mut file) = tokio::fs::File::open(path).await else { return StatusCode::NOT_FOUND.into_response() };
    let Ok(meta) = file.metadata().await else { return StatusCode::NOT_FOUND.into_response() };
    let size = meta.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split(',').next())
        .and_then(|r| r.split_once('-'))
        .and_then(|(a, b)| {
            if a.is_empty() {
                // "-500": die letzten 500 Bytes
                let n: u64 = b.parse().ok()?;
                Some((size.saturating_sub(n), size.saturating_sub(1)))
            } else {
                let start: u64 = a.parse().ok()?;
                let end = if b.is_empty() { size.saturating_sub(1) } else { b.parse::<u64>().ok()?.min(size.saturating_sub(1)) };
                Some((start, end))
            }
        });
    let (status, start, end) = match range {
        Some((s, e)) if s <= e && s < size => (StatusCode::PARTIAL_CONTENT, s, e.min(s + CHUNK - 1)),
        Some(_) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
        None if size > CHUNK => (StatusCode::PARTIAL_CONTENT, 0, CHUNK - 1),
        None => (StatusCode::OK, 0, size.saturating_sub(1)),
    };
    let len = if size == 0 { 0 } else { end - start + 1 };
    let mut buf = vec![0u8; len as usize];
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() || file.read_exact(&mut buf).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let mut resp = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, len)
        .header(header::CACHE_CONTROL, "private, max-age=3600");
    if status == StatusCode::PARTIAL_CONTENT {
        resp = resp.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    resp.body(Body::from(buf)).unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub async fn follows(State(state): State<AppState>, User(me): User) -> Json<Vec<SocialFollow>> {
    Json(state.db.social_follows(me.id))
}

pub async fn follow(State(state): State<AppState>, User(me): User, Json(f): Json<SocialFollow>) -> R<SocialFollow> {
    let p = platform_ok(&f.platform)?;
    let h = handle_ok(&p, &f.handle)?;
    // Name und Bild vom Profil – wenn das gerade nicht geht, trotzdem folgen
    let (name, avatar) = match state.social.profile(&p, &h, false).await {
        Ok(prof) => (prof.name.clone(), prof.avatar.clone()),
        Err(_) => (if f.name.is_empty() { h.clone() } else { f.name.clone() }, f.avatar.clone()),
    };
    let saved = SocialFollow { platform: p, handle: h, name, avatar, added: crate::db::now() };
    state.db.social_follow(me.id, &saved).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(saved))
}

pub async fn unfollow(State(state): State<AppState>, User(me): User, Path((p, h)): Path<(String, String)>) -> R<Value> {
    let p = platform_ok(&p)?;
    let h = handle_ok(&p, &h)?;
    state.db.social_unfollow(me.id, &p, &h).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn status(State(state): State<AppState>, User(_me): User) -> Json<Vec<SocialStatus>> {
    Json(state.social.status())
}

pub async fn admin_cookies(State(state): State<AppState>, Admin(_): Admin, Path(p): Path<String>, body: String) -> R<Value> {
    let p = platform_ok(&p)?;
    state.social.set_cookies(&p, &body).await.map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}
