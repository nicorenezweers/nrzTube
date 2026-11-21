//! nrzTube – privates YouTube-Frontend: ohne Werbung, ohne Google-Tracking, mit lokalen Abos,
//! Profilen, Playlists und Verlauf, SponsorBlock und DeArrow. Server in Rust (axum), Oberfläche
//! mit Dioxus/nrzUI als WebAssembly (eingebettet).

mod auth;
mod db;
mod feed;
mod import;
mod proxy;
mod reco;
mod routes;
mod sb;
mod yt;
mod ytdlp;
mod twitch;
mod twitch_routes;
mod ytchat;
mod party;
mod recap;
mod ytlive;
mod canireact;
mod social;
mod social_routes;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<db::Db>,
    pub yt: yt::Yt,
    pub ytdlp: Arc<ytdlp::YtDlp>,
    pub sb: Arc<sb::SponsorBlock>,
    pub feeds: Arc<feed::Feeds>,
    pub reco: Arc<reco::Reco>,
    pub twitch: Arc<twitch::Twitch>,
    pub parties: Arc<party::Parties>,
    pub ytlive: Arc<ytlive::YtLive>,
    pub cir: Arc<canireact::CanIReact>,
    pub social: Arc<social::Social>,
    pub media_http: reqwest::Client,
    #[allow(clippy::type_complexity)]
    pub trending: Arc<Mutex<HashMap<String, (Instant, Vec<nrztube_types::VideoItem>)>>>,
}

#[derive(rust_embed::RustEmbed)]
#[folder = "ui/dist/public"]
struct Spa;

fn env(key: &str, default: &str) -> String {
    std::env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

async fn spa(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({ "error": "Unbekannte Adresse" }))).into_response();
    }
    let (file, is_index) = match Spa::get(path).filter(|_| !path.is_empty()) {
        Some(f) => (f, false),
        None => match Spa::get("index.html") {
            Some(f) => (f, true),
            None => {
                return (StatusCode::SERVICE_UNAVAILABLE, "nrzTube: Oberfläche fehlt (scripts/ui.sh ausführen)").into_response();
            }
        },
    };
    let mime = if is_index { "text/html; charset=utf-8".to_string() } else { file.metadata.mimetype().to_string() };
    let mut resp = Response::new(Body::from(file.data.into_owned()));
    if let Ok(v) = HeaderValue::from_str(&mime) {
        resp.headers_mut().insert(header::CONTENT_TYPE, v);
    }
    // Von dx gehashte Dateien dürfen lange im Cache bleiben, index.html nie
    let cache = if is_index || path.ends_with("index.html") || path.ends_with(".webmanifest") || path.ends_with("sw.js") {
        "no-cache"
    } else if path.starts_with("assets/") || path.starts_with("wasm/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    resp
}

/// Sicherheits-Header für alle Antworten
async fn security_headers(req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-robots-tag", HeaderValue::from_static("noindex, nofollow"));
    resp
}

/// Bilder nur für angemeldete Konten (kein offener Bild-Proxy)
async fn require_session(state: axum::extract::State<AppState>, req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let mut ok = auth::token_from(&parts).and_then(|t| state.db.session_user(&t)).is_some();
    // Gäste einer Watch-Party dürfen Vorschaubilder laden
    if !ok {
        if let Some(c) = party::guest_cookie_from(&parts.headers) {
            ok = state.parties.valid_guest(&c).await;
        }
    }
    if !ok {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(Request::from_parts(parts, body)).await
}

fn load_secret(data: &std::path::Path) -> Vec<u8> {
    if let Ok(s) = std::env::var("NRZTUBE_SECRET") {
        if s.len() >= 16 {
            return s.into_bytes();
        }
    }
    let path = data.join("secret");
    if let Ok(s) = std::fs::read(&path) {
        if s.len() >= 32 {
            return s;
        }
    }
    let token = auth::new_token().into_bytes();
    let _ = std::fs::write(&path, &token);
    token
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = env("NRZTUBE_PORT", "8455").parse().unwrap_or(8455);
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        let ok = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/api/health"))
            .timeout(Duration::from_secs(4))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        std::process::exit(if ok { 0 } else { 1 });
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(env("NRZTUBE_LOG", "info")))
        .init();

    let data = PathBuf::from(env("NRZTUBE_DATA_DIR", "data"));
    std::fs::create_dir_all(&data)?;
    proxy::init_secret(load_secret(&data));

    let db = Arc::new(db::Db::open(&data.join("nrztube.db"))?);
    let yt = yt::Yt::new();
    let ytdlp = Arc::new(ytdlp::YtDlp::new(PathBuf::from(env("NRZTUBE_YTDLP", "yt-dlp"))));
    ytdlp.setup(&data).await;
    tracing::info!("yt-dlp {}", ytdlp.version.lock().await);
    let media_http = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36")
        .connect_timeout(Duration::from_secs(10))
        .build()?;
    let state = AppState {
        db: db.clone(),
        sb: Arc::new(sb::SponsorBlock::new(yt.http().clone())),
        feeds: Arc::new(feed::Feeds::new(yt.http().clone())),
        reco: Arc::new(reco::Reco::new(yt.http().clone())),
        twitch: twitch::Twitch::new(yt.http().clone()),
        parties: Arc::new(party::Parties::default()),
        ytlive: Arc::new(ytlive::YtLive::default()),
        cir: Arc::new(canireact::CanIReact::default()),
        social: social::Social::new(ytdlp.clone(), data.clone()),
        yt,
        ytdlp: ytdlp.clone(),
        media_http,
        trending: Arc::new(Mutex::new(HashMap::new())),
    };

    // Hintergrund: Feeds aller Abos alle 30 Minuten, yt-dlp täglich aktualisieren
    {
        let st = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            loop {
                let ids = st.db.all_subscribed_channels();
                if !ids.is_empty() {
                    st.feeds.many(&ids, true).await;
                }
                tokio::time::sleep(Duration::from_secs(1800)).await;
            }
        });
        // Wer von den Abos live ist: alle 4 Minuten prüfen
        let st2 = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            loop {
                let ids = st2.db.all_subscribed_channels();
                if !ids.is_empty() {
                    ytlive::YtLive::refresh(&st2, &ids, false).await;
                }
                tokio::time::sleep(Duration::from_secs(240)).await;
            }
        });
        let y = ytdlp.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(90)).await;
            loop {
                y.update().await;
                tokio::time::sleep(Duration::from_secs(86400)).await;
            }
        });
    }

    use routes::*;
    let api = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(change_password))
        .route("/api/admin", get(admin_info))
        .route("/api/admin/registration", post(admin_registration))
        .route("/api/admin/users", post(admin_create_user))
        .route("/api/admin/users/{id}", delete(admin_delete_user))
        .route("/api/admin/ytdlp", post(admin_update_ytdlp))
        .route("/api/admin/cookies", post(admin_cookies))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/search", get(search))
        .route("/api/suggest", get(suggest))
        .route("/api/resolve", get(resolve))
        .route("/api/trending", get(trending))
        .route("/api/video/{id}", get(video))
        .route("/api/video/{id}/streams", get(streams))
        .route("/api/video/{id}/external", get(external))
        .route("/api/comments", get(comments))
        .route("/api/channel/{id}", get(channel))
        .route("/api/channel/{id}/tab/{tab}", get(channel_tab))
        .route("/api/channel/{id}/search", get(channel_search))
        .route("/api/playlist/{id}", get(playlist))
        .route("/api/sponsorblock/{id}", get(sponsorblock))
        .route("/api/dearrow", get(dearrow))
        .route("/api/feed", get(feed))
        .route("/api/subscriptions", get(subscriptions).post(subscribe))
        .route("/api/subscriptions/refresh", post(refresh_subscription_thumbs))
        .route("/api/subscriptions/{id}", get(is_subscribed).delete(unsubscribe))
        .route("/api/profiles", get(profiles).post(save_profile))
        .route("/api/profiles/{id}", delete(delete_profile))
        .route("/api/playlists", get(playlists).post(create_playlist))
        .route("/api/playlists/containing/{vid}", get(playlists_with))
        .route("/api/playlists/{id}", get(local_playlist).put(update_playlist).delete(delete_playlist))
        .route("/api/playlists/{id}/videos", post(add_to_playlist))
        .route("/api/playlists/{id}/videos/{vid}", delete(remove_from_playlist))
        .route("/api/playlists/{id}/move", post(move_in_playlist))
        .route("/api/history", get(history).post(add_history).delete(clear_history))
        .route("/api/history/{vid}", get(history_entry).delete(delete_history_entry))
        .route("/api/rating/{id}", get(get_rating).post(rate))
        .route("/api/home", get(home))
        .route("/api/twitch/channel/{login}", get(twitch_routes::channel))
        .route("/api/twitch/channel/{login}/videos", get(twitch_routes::videos))
        .route("/api/twitch/search", get(twitch_routes::search))
        .route("/api/twitch/top", get(twitch_routes::top))
        .route("/api/twitch/games", get(twitch_routes::games))
        .route("/api/twitch/live", get(twitch_routes::live))
        .route("/api/twitch/details/{id}", get(twitch_routes::details))
        .route("/api/twitch/streams/{id}", get(twitch_routes::streams))
        .route("/api/twitch/nosub/{file}", get(twitch_routes::nosub))
        .route("/api/twitch/chat/{login}", get(twitch_routes::chat).post(twitch_routes::twitch_chat_send))
        .route("/api/ytchat/{video}", get(twitch_routes::yt_chat).post(twitch_routes::yt_chat_send))
        .route("/api/canireact/{id}", get(canireact::info))
        .route("/api/youtube/live", get(ytlive::live))
        .route("/api/recap", get(recap::recap))
        .route("/api/recap/channel/{id}", get(recap::channel))
        .route("/api/recap/video/{id}", get(recap::video))
        .route("/api/accounts", get(twitch_routes::accounts))
        .route("/api/invites", get(list_invites).post(create_invite))
        .route("/api/invites/{code}", delete(delete_invite).get(check_invite))
        .route("/api/party", post(party::create))
        .route("/api/party/mine", get(party::mine))
        .route("/api/party/public", get(party::public_list))
        .route("/api/party/{id}", get(party::info))
        .route("/api/party/{id}/join", post(party::join))
        .route("/api/party/{id}/events", get(party::events))
        .route("/api/party/{id}/action", post(party::action))
        .route("/api/party/{id}/leave", post(party::leave))
        .route("/api/party/{id}/streams", get(party::streams))
        .route("/api/party/{id}/resolve", get(party::resolve))
        .route("/api/accounts/twitch/start", post(twitch_routes::twitch_login_start))
        .route("/api/accounts/twitch/poll", post(twitch_routes::twitch_login_poll))
        .route("/api/accounts/twitch/import", post(twitch_routes::twitch_import_follows))
        .route("/api/accounts/twitch", delete(twitch_routes::twitch_logout))
        .route("/api/accounts/youtube", post(twitch_routes::youtube_account_set))
        .route("/api/twitch/follows", get(twitch_routes::follows).post(twitch_routes::follow))
        .route("/api/twitch/follows/{login}", delete(twitch_routes::unfollow))
        .route("/api/links/youtube/{id}", get(twitch_routes::youtube_links))
        .route("/api/links", post(twitch_routes::add_link))
        .route("/api/links/{yt}/{tw}", delete(twitch_routes::remove_link))
        .route("/api/social/feed", get(social_routes::feed))
        .route("/api/social/status", get(social_routes::status))
        .route("/api/social/profile/{platform}/{handle}", get(social_routes::profile))
        .route("/api/social/post/{platform}/{id}", get(social_routes::post))
        .route("/api/social/follows", get(social_routes::follows).post(social_routes::follow))
        .route("/api/social/follows/{platform}/{handle}", delete(social_routes::unfollow))
        .route("/api/admin/social-cookies/{platform}", post(social_routes::admin_cookies))
        .route("/api/taste", get(taste).delete(reset_taste))
        .route("/api/prefetch", post(prefetch))
        .route("/api/export/subscriptions", get(export_subscriptions))
        .route("/api/import/subscriptions", post(import_subscriptions))
        .route("/api/export/playlists", get(export_playlists))
        .route("/api/import/playlists", post(import_playlists))
        .route("/api/export/history", get(export_history))
        .route("/api/import/history", post(import_history))
        .layer(tower_http::compression::CompressionLayer::new());

    let images = Router::new()
        .route("/img/{token}", get(proxy::image))
        .route("/vi/{id}/{file}", get(proxy::video_image))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_session));

    let app = Router::new()
        .merge(api)
        .merge(images)
        .route("/p/{token}", get(proxy::media_proxy))
        // Videos aus dem Social-Cache: ohne Komprimierung (Range-Anfragen)
        .route("/api/social/play/{platform}/{id}", get(social_routes::play))
        .fallback(get(spa).layer(tower_http::compression::CompressionLayer::new()))
        .layer(middleware::from_fn(security_headers))
        .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state);

    let addr = format!("{}:{port}", env("NRZTUBE_ADDRESS", "0.0.0.0"));
    tracing::info!("nrzTube {} auf http://{addr}", env!("CARGO_PKG_VERSION"));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
