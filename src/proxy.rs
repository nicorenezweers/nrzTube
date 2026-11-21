//! Alles, was der Browser von YouTube braucht, holt nrzTube selbst: Vorschaubilder, Avatare,
//! Videos (HLS-Listen werden umgeschrieben), Untertitel. Der Browser spricht nie mit Google.
//!
//! - `/img/<base64url>` – Bilder, nur von YouTubes Bild-Servern (Allowlist)
//! - `/vi/<id>/<datei>` – Video-Vorschaubilder von i.ytimg.com
//! - `/p/<base64url>.<sig>` – Medien (googlevideo, Untertitel), mit HMAC signiert: nur
//!   Adressen, die der Server selbst ausgegeben hat, lassen sich abrufen. So funktionieren
//!   sie auch in externen Playern ohne Anmeldung.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use futures::TryStreamExt;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::OnceLock;

use crate::AppState;

static SECRET: OnceLock<Vec<u8>> = OnceLock::new();

pub fn init_secret(secret: Vec<u8>) {
    let _ = SECRET.set(secret);
}

fn mac(data: &str) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(SECRET.get().map(Vec::as_slice).unwrap_or(b"nrztube"))
        .expect("hmac");
    m.update(data.as_bytes());
    B64.encode(&m.finalize().into_bytes()[..16])
}

const IMG_HOSTS: &[&str] = &[
    "i.ytimg.com",
    "i1.ytimg.com",
    "i2.ytimg.com",
    "i3.ytimg.com",
    "i4.ytimg.com",
    "i9.ytimg.com",
    "yt3.ggpht.com",
    "yt3.googleusercontent.com",
    "yt4.ggpht.com",
    "lh3.googleusercontent.com",
    "dearrow-thumb.ajay.app",
    "static-cdn.jtvnw.net",
    "clips-media-assets2.twitch.tv",
    "vod-secure.twitch.tv",
    "cdn.7tv.app",
    "cdn.betterttv.net",
    "cdn.frankerfacez.com",
];

/// Bild-Server von TikTok, Instagram und X (wechselnde Unterdomains)
fn social_img_host(h: &str) -> bool {
    h == "pbs.twimg.com"
        || h.ends_with(".cdninstagram.com")
        || h.ends_with(".fbcdn.net")
        || h.ends_with(".tiktokcdn.com")
        || h.ends_with(".tiktokcdn-eu.com")
        || h.ends_with(".tiktokcdn-us.com")
        || h.ends_with(".ibyteimg.com")
}

fn host_of(u: &str) -> Option<&str> {
    let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"))?;
    Some(rest.split(['/', '?', ':']).next().unwrap_or(""))
}

fn media_host_ok(u: &str) -> bool {
    match host_of(u) {
        Some(h) => {
            h.ends_with(".googlevideo.com")
                || h.ends_with(".ttvnw.net")
                || h.ends_with(".cloudfront.net")
                || h.ends_with(".twitchcdn.net")
                || h.ends_with(".twitch.tv")
                || h.ends_with(".jtvnw.net")
                || h == "video.twimg.com"
                || h.ends_with(".cdninstagram.com")
                || h.ends_with(".fbcdn.net")
                || h == "www.youtube.com" && u.contains("/api/timedtext")
                || h.ends_with(".youtube.com") && u.contains("/api/timedtext")
        }
        None => false,
    }
}

/// Bild-Adresse über den nrzTube-Server
pub fn img(u: &str) -> String {
    let u = crate::yt::json::fix_url(u);
    if let Some((id, file)) = ytimg_video_id(&u) {
        return format!("/vi/{id}/{file}");
    }
    format!("/img/{}", B64.encode(u.as_bytes()))
}

fn ytimg_video_id(u: &str) -> Option<(String, String)> {
    let rest = u.strip_prefix("https://i.ytimg.com/vi/")?;
    let mut parts = rest.split('/');
    let id = parts.next()?;
    let full = parts.next()?;
    // Kapitel- und Storyboard-Bilder (hqdefault_5933.jpg?sqp=…&rs=…) gibt es nur mit den
    // signierten Parametern – die gehen über /img/ samt Abfrage.
    if full.contains('?') && full.split('?').next()?.contains('_') {
        return None;
    }
    let file = full.split('?').next()?;
    let ok = file.len() < 40 && file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
    (id.len() == 11 && ok).then(|| (id.to_string(), file.to_string()))
}

/// Vorschaubild eines Videos (hqdefault gibt es immer; die Oberfläche schneidet auf 16:9 zu)
pub fn video_thumb(id: &str) -> String {
    format!("/vi/{id}/hqdefault.jpg")
}

/// Signierte Medien-Adresse
pub fn media(u: &str) -> String {
    let p = B64.encode(u.as_bytes());
    let sig = mac(&p);
    format!("/p/{p}.{sig}")
}

fn decode_media(token: &str) -> Option<String> {
    let (p, sig) = token.rsplit_once('.')?;
    // Konstante Zeit ist hier nicht kritisch, aber billig
    let expect = mac(p);
    if expect.len() != sig.len() || !expect.bytes().zip(sig.bytes()).all(|(a, b)| a == b) {
        return None;
    }
    let u = String::from_utf8(B64.decode(p).ok()?).ok()?;
    media_host_ok(&u).then_some(u)
}

fn cache_headers(resp: &mut Response<Body>, secs: u32) {
    if let Ok(v) = HeaderValue::from_str(&format!("private, max-age={secs}")) {
        resp.headers_mut().insert(header::CACHE_CONTROL, v);
    }
}

async fn fetch_image(state: &AppState, url: &str) -> Response<Body> {
    let res = match state.yt.http().get(url).send().await {
        Ok(r) => r,
        Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
    };
    if !res.status().is_success() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let ct = res
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or(HeaderValue::from_static("image/jpeg"));
    let stream = res.bytes_stream().map_err(std::io::Error::other);
    let mut resp = Response::new(Body::from_stream(stream));
    resp.headers_mut().insert(header::CONTENT_TYPE, ct);
    cache_headers(&mut resp, 86400 * 7);
    resp
}

pub async fn image(State(state): State<AppState>, Path(token): Path<String>) -> Response<Body> {
    let token = token.split('.').next().unwrap_or("");
    let Some(url) = B64.decode(token).ok().and_then(|b| String::from_utf8(b).ok()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match host_of(&url) {
        Some(h) if IMG_HOSTS.contains(&h) || social_img_host(h) => fetch_image(&state, &url).await,
        _ => StatusCode::FORBIDDEN.into_response(),
    }
}

pub async fn video_image(State(state): State<AppState>, Path((id, file)): Path<(String, String)>) -> Response<Body> {
    let ok_id = id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let ok_file = file.len() < 40 && file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
    if !ok_id || !ok_file {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let host = if file.ends_with(".webp") { "https://i.ytimg.com/vi_webp" } else { "https://i.ytimg.com/vi" };
    fetch_image(&state, &format!("{host}/{id}/{file}")).await
}

#[derive(serde::Deserialize, Default)]
pub struct MediaQuery {
    /// HLS: nur H.264-Varianten behalten
    #[serde(default)]
    codec: Option<String>,
    /// HLS: höchste Auflösung
    #[serde(default)]
    max: Option<u32>,
    /// Twitch-Live: Kanal und Auflösung dieser Variante (für Ersatzlisten bei Werbung)
    #[serde(default)]
    tw: Option<String>,
    #[serde(default)]
    res: Option<String>,
}

/// Medien-Proxy: HLS-Listen werden umgeschrieben, alles andere wird mit Range durchgereicht.
pub fn playlist_response(body: String) -> Response<Body> {
    let mut resp = Response::new(Body::from(body));
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/vnd.apple.mpegurl"));
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    resp
}

pub async fn media_proxy(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(q): Query<MediaQuery>,
    headers: HeaderMap,
) -> Response<Body> {
    let token = token.trim_end_matches(".m3u8").trim_end_matches(".ts").trim_end_matches(".mp4");
    let Some(url) = decode_media(token) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let mut req = state.media_http.get(&url);
    if let Some(r) = headers.get(header::RANGE) {
        req = req.header(header::RANGE, r);
    }
    let res = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("Medien-Proxy: {e}");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let status = res.status();
    let ct = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let path_m3u8 = url.split('?').next().unwrap_or("").ends_with(".m3u8");
    let is_playlist = ct.contains("mpegurl") || url.contains("/api/manifest/hls_") || path_m3u8;
    if is_playlist && status.is_success() {
        let text = match res.text().await {
            Ok(t) => t,
            Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
        };
        let mut base = url.clone();
        let mut text = text;
        if host_of(&url).is_some_and(|h| h.ends_with(".ttvnw.net")) {
            // Twitch-Live mit Werbung: Ersatzliste eines anderen Player-Typs (wie vaft),
            // sonst die Werbeabschnitte herausschneiden
            if crate::twitch::has_ads(&text) {
                if let Some(login) = q.tw.as_deref().filter(|l| crate::twitch::valid_login(l)) {
                    if let Some((u, t)) = state.twitch.backup_media(login, q.res.as_deref().unwrap_or("")).await {
                        base = u;
                        text = t;
                    }
                }
            }
            text = crate::twitch::strip_ads(&text);
            // Hauptliste: Kanal und Auflösung an die Varianten hängen
            if let Some(login) = crate::twitch::usher_channel(&url) {
                let body = crate::twitch::tag_variants(&rewrite_m3u8(&text, &base, q.codec.as_deref(), q.max), &login);
                return playlist_response(body);
            }
        }
        // Twitch-VODs: stummgeschaltete Abschnitte heissen in der Ablage "-muted"
        if host_of(&url).is_some_and(|h| h.ends_with(".cloudfront.net")) && text.contains("-unmuted") {
            text = text.replace("-unmuted", "-muted");
        }
        let body = rewrite_m3u8(&text, &base, q.codec.as_deref(), q.max);
        let mut resp = Response::new(Body::from(body));
        resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/vnd.apple.mpegurl"));
        resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        resp.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
        return resp;
    }
    let mut resp = Response::builder().status(status.as_u16());
    for h in [header::CONTENT_TYPE, header::CONTENT_LENGTH, header::CONTENT_RANGE, header::ACCEPT_RANGES, header::LAST_MODIFIED] {
        if let Some(v) = res.headers().get(&h) {
            resp = resp.header(h, v.clone());
        }
    }
    resp = resp.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    if url.contains("/api/timedtext") {
        resp = resp.header(header::CACHE_CONTROL, "private, max-age=3600");
    }
    let stream = res.bytes_stream().map_err(std::io::Error::other);
    resp.body(Body::from_stream(stream)).unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn resolve(base: &str, rel: &str) -> String {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    if let Some(stripped) = rel.strip_prefix('/') {
        let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
        let host_end = base[scheme_end..].find('/').map(|i| i + scheme_end).unwrap_or(base.len());
        return format!("{}/{}", &base[..host_end], stripped);
    }
    let dir = base.rsplit_once('/').map(|(d, _)| d).unwrap_or(base);
    format!("{dir}/{rel}")
}

fn attr<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=");
    let start = line.find(&key)? + key.len();
    let rest = &line[start..];
    if let Some(r) = rest.strip_prefix('"') {
        r.split('"').next()
    } else {
        rest.split(',').next()
    }
}

/// Schreibt alle Adressen einer HLS-Liste auf den Proxy um. In der Hauptliste werden
/// eingebettete Untertitel entfernt (nrzTube bringt eigene) und auf Wunsch VP9/AV1 und zu
/// hohe Auflösungen weggelassen.
pub fn rewrite_m3u8(text: &str, base: &str, codec: Option<&str>, max: Option<u32>) -> String {
    let mut out = String::with_capacity(text.len());
    let lines: Vec<&str> = text.lines().collect();
    let mut skip_next = false;
    let only_avc = codec == Some("avc1");
    let has_avc = text.contains("avc1");
    for line in lines.iter() {
        if skip_next {
            skip_next = false;
            if !line.starts_with('#') {
                continue;
            }
        }
        if line.starts_with("#EXT-X-MEDIA:") && line.contains("TYPE=SUBTITLES") {
            continue;
        }
        if line.starts_with("#EXT-X-STREAM-INF:") {
            let codecs = attr(line, "CODECS").unwrap_or("");
            let height = attr(line, "RESOLUTION")
                .and_then(|r| r.split('x').nth(1))
                .and_then(|h| h.parse::<u32>().ok())
                .unwrap_or(0);
            let drop_codec = only_avc && has_avc && !codecs.contains("avc1");
            let drop_height = max.is_some_and(|m| height > m);
            if drop_codec || drop_height {
                skip_next = true;
                continue;
            }
            let cleaned = line.replace(",SUBTITLES=\"vtt\"", "");
            out.push_str(&cleaned);
            out.push('\n');
            continue;
        }
        if line.starts_with('#') {
            if let Some(uri) = attr(line, "URI") {
                if line.contains("URI=\"") {
                    let abs = resolve(base, uri);
                    out.push_str(&line.replace(&format!("URI=\"{uri}\""), &format!("URI=\"{}\"", media(&abs))));
                    out.push('\n');
                    continue;
                }
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if line.trim().is_empty() {
            out.push('\n');
            continue;
        }
        out.push_str(&media(&resolve(base, line.trim())));
        out.push('\n');
    }
    out
}
