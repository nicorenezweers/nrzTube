//! API für Twitch und die Verknüpfung von YouTube- und Twitch-Kanälen.
//!
//! Verknüpft wird, wenn ein Kanal den anderen verlinkt (Social-Links bei Twitch, „Über“-Links
//! oder Beschreibung bei YouTube, Videobeschreibungen) oder – vorsichtiger – bei gleichem Namen
//! und genug Followern. Was einmal gefunden ist, liegt in der Datenbank; von Hand Gelöstes
//! kommt nicht wieder.

use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use nrztube_types::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::time::Duration;

use crate::auth::User;
use crate::routes::{bad, keep_twitch, ApiError, R};
use crate::twitch::{self, live_item, norm, similar, twitch_logins_in, valid_login, youtube_urls};
use crate::yt::Locale;
use crate::AppState;

const RECHECK: i64 = 3 * 86400;

fn login_ok(l: &str) -> Result<String, ApiError> {
    let l = l.trim().trim_start_matches('@').to_lowercase();
    if valid_login(&l) { Ok(l) } else { Err(bad("Ungültiger Twitch-Kanal")) }
}

fn yt_id_ok(id: &str) -> bool {
    id.starts_with("UC") && id.len() == 24 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ------------------------------------------------------------------ Twitch

pub async fn channel(State(state): State<AppState>, User(_me): User, Path(login): Path<String>) -> R<TwitchChannel> {
    let login = login_ok(&login)?;
    let mut ch = state.twitch.channel(&login).await?;
    ch.links = links_for_twitch(&state, &ch).await;
    Ok(Json(ch))
}

#[derive(Deserialize, Default)]
pub struct KindQ {
    #[serde(default)]
    kind: String,
}

pub async fn videos(State(state): State<AppState>, User(me): User, Path(login): Path<String>, Query(q): Query<KindQ>) -> R<Vec<VideoItem>> {
    let login = login_ok(&login)?;
    let s = state.db.settings(me.id);
    // Auf der Kanalseite selbst nur Wörter und Kategorien filtern, nicht den Kanal
    let mut s2 = s.clone();
    s2.hidden_channels.clear();
    Ok(Json(state.twitch.videos(&login, &q.kind).await?.into_iter().filter(|v| keep_twitch(&s2, v)).collect()))
}

#[derive(Deserialize, Default)]
pub struct SearchQ {
    #[serde(default)]
    q: String,
    #[serde(default)]
    game: String,
}

pub async fn search(State(state): State<AppState>, User(me): User, Query(q): Query<SearchQ>) -> R<Vec<TwitchChannel>> {
    let q = q.q.trim();
    if q.is_empty() || q.len() > 100 {
        return Ok(Json(vec![]));
    }
    let s = state.db.settings(me.id);
    Ok(Json(
        state
            .twitch
            .search(q)
            .await?
            .into_iter()
            .filter(|c| !s.hidden_channels.iter().any(|h| h.eq_ignore_ascii_case(&format!("tw:{}", c.login)) || h.eq_ignore_ascii_case(&c.login) || h.eq_ignore_ascii_case(&c.name)))
            .collect(),
    ))
}

pub async fn top(State(state): State<AppState>, User(me): User, Query(q): Query<SearchQ>) -> R<Vec<VideoItem>> {
    let s = state.db.settings(me.id);
    Ok(Json(state.twitch.top(Some(q.game.as_str())).await?.into_iter().filter(|v| keep_twitch(&s, v)).collect()))
}

pub async fn games(State(state): State<AppState>, User(me): User) -> R<Vec<Value>> {
    let s = state.db.settings(me.id);
    Ok(Json(
        state
            .twitch
            .games()
            .await?
            .into_iter()
            .filter(|(name, _, _)| !s.twitch_hidden_games.iter().any(|g| g.trim().eq_ignore_ascii_case(name)))
            .map(|(name, art, viewers)| json!({ "name": name, "art": art, "viewers": viewers }))
            .collect(),
    ))
}

fn tw_id_ok(id: &str) -> Result<(), ApiError> {
    let ok = id.len() < 120 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-');
    if is_twitch_id(id) && ok { Ok(()) } else { Err(bad("Ungültige Twitch-ID")) }
}

pub async fn details(State(state): State<AppState>, User(_me): User, Path(id): Path<String>) -> R<VideoDetails> {
    tw_id_ok(&id)?;
    Ok(Json(state.twitch.details(&id).await?))
}

pub async fn streams(State(state): State<AppState>, User(_me): User, Path(id): Path<String>) -> R<StreamInfo> {
    tw_id_ok(&id)?;
    Ok(Json(state.twitch.streams(&id).await?))
}

/// Chat als Server-Sent Events
pub async fn chat(State(state): State<AppState>, User(_me): User, Path(login): Path<String>) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let login = login_ok(&login)?;
    let (tx, rx) = tokio::sync::mpsc::channel::<ChatLine>(256);
    let tw = state.twitch.clone();
    tokio::spawn(async move {
        if let Err(e) = twitch::chat(tw, login, tx).await {
            tracing::debug!("Twitch-Chat: {e}");
        }
    });
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        let m = rx.recv().await?;
        let ev = Event::default().json_data(&m).unwrap_or_else(|_| Event::default());
        Some((Ok(ev), rx))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20))))
}

pub async fn follows(State(state): State<AppState>, User(me): User) -> Json<Vec<TwitchFollow>> {
    Json(state.db.twitch_follows(me.id))
}

pub async fn follow(State(state): State<AppState>, User(me): User, Json(f): Json<TwitchFollow>) -> R<Value> {
    let login = login_ok(&f.login)?;
    let (name, thumb) = match state.twitch.channel(&login).await {
        Ok(c) => (c.name, c.avatar),
        Err(_) => (f.name.clone(), f.thumb.clone()),
    };
    state.db.twitch_follow(me.id, &TwitchFollow { login, name, thumb, added: 0 }).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn unfollow(State(state): State<AppState>, User(me): User, Path(login): Path<String>) -> R<Value> {
    let login = login_ok(&login)?;
    state.db.twitch_unfollow(me.id, &login).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Wer gerade live ist: gefolgte Twitch-Kanäle und die Twitch-Kanäle abonnierter YouTuber
pub async fn live(State(state): State<AppState>, User(me): User) -> Json<Vec<VideoItem>> {
    let mut logins: Vec<String> = state.db.twitch_follows(me.id).into_iter().map(|f| f.login).collect();
    let subs: Vec<String> = state.db.subscriptions(me.id).into_iter().map(|s| s.channel_id).collect();
    for (_, tw) in state.db.twitch_for_youtube_many(&subs) {
        if !logins.contains(&tw) {
            logins.push(tw);
        }
    }
    let mut out: Vec<VideoItem> = state
        .twitch
        .live_many(&logins)
        .await
        .into_iter()
        .filter_map(|c| c.live.as_ref().map(|st| live_item(&c.login, &c.name, &c.avatar, st)))
        .collect();
    let s = state.db.settings(me.id);
    out.retain(|v| keep_twitch(&s, v));
    out.sort_by_key(|v| std::cmp::Reverse(v.views.unwrap_or(0)));
    Json(out)
}

// ------------------------------------------------------------------ Verknüpfung

/// YouTube-Adresse -> Kanal-ID (mit Zwischenspeicher in der App-Tabelle)
async fn resolve_yt(state: &AppState, url: &str) -> Option<String> {
    let key = format!("ytres:{}", url.to_lowercase().trim_end_matches('/'));
    if let Some(v) = state.db.app_setting(&key) {
        return Some(v).filter(|v| !v.is_empty());
    }
    let id = state.yt.channel_id(url, &Locale::default()).await.ok().filter(|id| yt_id_ok(id));
    let _ = state.db.set_app_setting(&key, id.as_deref().unwrap_or(""));
    id
}

/// Kanal-IDs, auf die ein Twitch-Kanal verlinkt
async fn twitch_backlinks(state: &AppState, ch: &TwitchChannel) -> Vec<String> {
    let mut out = vec![];
    for u in youtube_urls(&ch.socials, &ch.description).into_iter().take(6) {
        if let Some(id) = resolve_yt(state, &u).await {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

async fn with_live(state: &AppState, mut links: Vec<ChannelLink>) -> Vec<ChannelLink> {
    if links.is_empty() {
        return links;
    }
    let logins: Vec<String> = links.iter().map(|l| l.tw_login.clone()).collect();
    let live = state.twitch.live_many(&logins).await;
    for l in links.iter_mut() {
        if let Some(c) = live.iter().find(|c| c.login == l.tw_login) {
            l.live = c.live.clone();
            if l.tw_thumb.is_empty() {
                l.tw_thumb = c.avatar.clone();
            }
            if l.tw_name.is_empty() {
                l.tw_name = c.name.clone();
            }
        }
    }
    links
}

/// Twitch-Kanal -> YouTube: Social-Links auswerten (alle paar Tage)
async fn links_for_twitch(state: &AppState, ch: &TwitchChannel) -> Vec<ChannelLink> {
    let key = format!("tw:{}", ch.login);
    if !state.db.link_checked_recently(&key, RECHECK) {
        for yt in twitch_backlinks(state, ch).await {
            if state.db.is_rejected(&yt, &ch.login) {
                continue;
            }
            let (yt_name, yt_thumb) = match state.yt.channel(&yt, &Locale::default()).await {
                Ok(c) => (c.name, c.avatar),
                Err(_) => (String::new(), String::new()),
            };
            let _ = state.db.save_link(&ChannelLink {
                yt_id: yt,
                yt_name,
                yt_thumb,
                tw_login: ch.login.clone(),
                tw_name: ch.name.clone(),
                tw_thumb: ch.avatar.clone(),
                source: "twitch".into(),
                live: None,
            });
        }
        state.db.mark_link_checked(&key);
    }
    let mut links = state.db.links_for_twitch(&ch.login);
    for l in links.iter_mut() {
        l.live = ch.live.clone();
    }
    links
}

/// Prüft, ob `login` zu diesem YouTube-Kanal gehört, und speichert die Verknüpfung
async fn try_link(state: &AppState, yt: &Channel, login: &str, source: &str, need_proof: bool) -> bool {
    if state.db.is_rejected(&yt.id, login) {
        return false;
    }
    let Ok(tw) = state.twitch.channel(login).await else { return false };
    let names = [yt.name.as_str(), yt.handle.as_deref().unwrap_or("").trim_start_matches('@')];
    let name_match = names.iter().any(|n| !n.is_empty() && (similar(n, &tw.login) || similar(n, &tw.name)));
    let backlink = twitch_backlinks(state, &tw).await.contains(&yt.id);
    let ok = match source {
        // Der Kanal selbst verlinkt: genügt, wenn der Name passt oder Twitch zurückverlinkt
        "youtube" => !need_proof || name_match || backlink,
        "video" => name_match || backlink,
        // Nur gleicher Name: Rückverlinkung oder ein grosser Kanal
        _ => backlink || name_match && tw.followers.unwrap_or(0) >= 1000,
    };
    if ok {
        let src = if backlink && source == "name" { "twitch" } else { source };
        let _ = state.db.save_link(&ChannelLink {
            yt_id: yt.id.clone(),
            yt_name: yt.name.clone(),
            yt_thumb: yt.avatar.clone(),
            tw_login: tw.login.clone(),
            tw_name: tw.name.clone(),
            tw_thumb: tw.avatar.clone(),
            source: src.into(),
            live: None,
        });
    }
    ok
}

#[derive(Deserialize, Default)]
pub struct LinkQ {
    /// Twitch-Logins aus Videobeschreibungen dieses Kanals (von der Oberfläche gefunden)
    #[serde(default)]
    hint: String,
}

/// Twitch-Kanäle zu einem YouTube-Kanal (sucht bei Bedarf)
pub async fn youtube_links(State(state): State<AppState>, User(_me): User, Path(yt): Path<String>, Query(q): Query<LinkQ>) -> R<Vec<ChannelLink>> {
    if !yt_id_ok(&yt) {
        return Err(bad("Ungültige Kanal-ID"));
    }
    let hints: Vec<String> = q.hint.split(',').map(|h| h.trim().to_lowercase()).filter(|h| valid_login(h)).take(4).collect();
    let key = format!("yt:{yt}");
    let fresh = !state.db.link_checked_recently(&key, RECHECK);
    let known = state.db.links_for_youtube(&yt);
    let new_hints: Vec<&String> = hints.iter().filter(|h| !known.iter().any(|k| &k.tw_login == *h)).collect();
    if fresh || !new_hints.is_empty() {
        if let Ok(ch) = state.yt.channel(&yt, &Locale::default()).await {
            let mut found = false;
            for h in new_hints {
                found |= try_link(&state, &ch, h, "video", true).await;
            }
            if fresh {
                // 1. Der YouTube-Kanal verlinkt Twitch („Über“-Links, Beschreibung)
                let about = state.yt.channel_about_links(&yt, &Locale::default()).await.unwrap_or_default();
                let about_text: String = about.iter().map(|(_, u)| u.as_str()).collect::<Vec<_>>().join(" ");
                let from_about = twitch_logins_in(&about_text);
                let from_desc = twitch_logins_in(&ch.description);
                for l in from_about.iter().take(3) {
                    found |= try_link(&state, &ch, l, "youtube", from_about.len() > 1).await;
                }
                for l in from_desc.iter().filter(|l| !from_about.contains(l)).take(3) {
                    found |= try_link(&state, &ch, l, "youtube", true).await;
                }
                // 2. Gleicher Name (mit Rückverlinkung oder genug Followern)
                if !found && known.is_empty() {
                    let mut cands: Vec<String> = vec![norm(&ch.name)];
                    if let Some(h) = &ch.handle {
                        cands.push(norm(h.trim_start_matches('@')));
                    }
                    cands.retain(|c| valid_login(c) && c.len() >= 3);
                    cands.dedup();
                    for c in cands {
                        if try_link(&state, &ch, &c, "name", false).await {
                            break;
                        }
                    }
                }
                state.db.mark_link_checked(&key);
            }
        }
    }
    Ok(Json(with_live(&state, state.db.links_for_youtube(&yt)).await))
}

/// Von Hand verknüpfen
pub async fn add_link(State(state): State<AppState>, User(_me): User, Json(b): Json<LinkBody>) -> R<Value> {
    if !yt_id_ok(&b.yt_id) {
        return Err(bad("Ungültige Kanal-ID"));
    }
    let login = login_ok(&b.tw_login)?;
    let tw = state.twitch.channel(&login).await?;
    let yt = state.yt.channel(&b.yt_id, &Locale::default()).await.ok();
    state
        .db
        .save_link(&ChannelLink {
            yt_id: b.yt_id.clone(),
            yt_name: yt.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            yt_thumb: yt.as_ref().map(|c| c.avatar.clone()).unwrap_or_default(),
            tw_login: tw.login,
            tw_name: tw.name,
            tw_thumb: tw.avatar,
            source: "manual".into(),
            live: None,
        })
        .map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Verknüpfung lösen (kommt auch bei der automatischen Suche nicht wieder)
pub async fn remove_link(State(state): State<AppState>, User(_me): User, Path((yt, tw)): Path<(String, String)>) -> R<Value> {
    let login = login_ok(&tw)?;
    state.db.reject_link(&yt, &login).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ Verbundene Konten

fn sse_from(rx: tokio::sync::mpsc::Receiver<ChatLine>) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        let m = rx.recv().await?;
        let ev = Event::default().json_data(&m).unwrap_or_else(|_| Event::default());
        Some((Ok(ev), rx))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}

/// Twitch-Anmeldung des Nutzers, bei Bedarf erneuert
async fn twitch_auth(state: &AppState, user: i64) -> Result<twitch::TwAuth, ApiError> {
    let raw = state.db.account(user, "twitch").ok_or_else(|| bad("Zum Schreiben bitte erst unter Einstellungen → Twitch mit Twitch anmelden"))?;
    let a: twitch::TwAuth = serde_json::from_str(&raw).map_err(|_| bad("Twitch-Anmeldung beschädigt – bitte neu verbinden"))?;
    match state.twitch.fresh(&a).await {
        Ok(Some(n)) => {
            let _ = state.db.set_account(user, "twitch", &serde_json::to_string(&n).unwrap_or_default());
            Ok(n)
        }
        Ok(None) => Ok(a),
        Err(e) => Err(bad(&e.to_string())),
    }
}

pub async fn accounts(State(state): State<AppState>, User(me): User) -> Json<AccountsInfo> {
    let twitch = state.db.account(me.id, "twitch").and_then(|r| serde_json::from_str::<twitch::TwAuth>(&r).ok()).map(|a| a.login);
    Json(AccountsInfo { twitch, youtube: state.db.account(me.id, "youtube").is_some() })
}

pub async fn twitch_login_start(State(state): State<AppState>, User(_me): User) -> R<DeviceCode> {
    let v = state.twitch.device_start().await?;
    Ok(Json(serde_json::from_value(v).map_err(|e| bad(&e.to_string()))?))
}

#[derive(Deserialize)]
pub struct PollBody {
    device_code: String,
}

pub async fn twitch_login_poll(State(state): State<AppState>, User(me): User, Json(b): Json<PollBody>) -> R<Value> {
    match state.twitch.device_poll(&b.device_code).await? {
        None => Ok(Json(json!({ "status": "pending" }))),
        Some(a) => {
            state.db.set_account(me.id, "twitch", &serde_json::to_string(&a).unwrap_or_default()).map_err(|e| bad(&e.to_string()))?;
            Ok(Json(json!({ "status": "ok", "login": a.login })))
        }
    }
}

pub async fn twitch_logout(State(state): State<AppState>, User(me): User) -> R<Value> {
    state.db.delete_account(me.id, "twitch").map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Gefolgte Kanäle des Twitch-Kontos übernehmen
pub async fn twitch_import_follows(State(state): State<AppState>, User(me): User) -> R<Value> {
    let a = twitch_auth(&state, me.id).await?;
    let list = state.twitch.followed(&a).await?;
    let have: std::collections::HashSet<String> = state.db.twitch_follows(me.id).into_iter().map(|f| f.login).collect();
    let mut added = 0;
    for (login, name) in list {
        if valid_login(&login) && !have.contains(&login) {
            let _ = state.db.twitch_follow(me.id, &TwitchFollow { login, name, thumb: String::new(), added: 0 });
            added += 1;
        }
    }
    // Profilbilder nachladen
    let st = state.clone();
    let uid = me.id;
    tokio::spawn(async move {
        let missing: Vec<String> = st.db.twitch_follows(uid).into_iter().filter(|f| f.thumb.is_empty()).map(|f| f.login).collect();
        for c in st.twitch.live_many(&missing).await {
            let _ = st.db.twitch_follow(uid, &TwitchFollow { login: c.login, name: c.name, thumb: c.avatar, added: 0 });
        }
    });
    Ok(Json(json!({ "added": added })))
}

pub async fn youtube_account_set(State(state): State<AppState>, User(me): User, body: String) -> R<Value> {
    if body.trim().is_empty() {
        state.db.delete_account(me.id, "youtube").map_err(|e| bad(&e.to_string()))?;
        return Ok(Json(json!({ "ok": true })));
    }
    crate::ytchat::YtAuth::from_netscape(&body).map_err(|e| bad(&e.to_string()))?;
    state.db.set_account(me.id, "youtube", &body).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn twitch_chat_send(State(state): State<AppState>, User(me): User, Path(login): Path<String>, Json(b): Json<ChatSend>) -> R<Value> {
    let login = login_ok(&login)?;
    let a = twitch_auth(&state, me.id).await?;
    state.twitch.send_chat(me.id, &a, &login, &b.text).await.map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

fn video_ok(id: &str) -> Result<(), ApiError> {
    if id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') { Ok(()) } else { Err(bad("Ungültige Video-ID")) }
}

/// YouTube-Livechat als Server-Sent Events
pub async fn yt_chat(State(state): State<AppState>, User(_me): User, Path(video): Path<String>) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    video_ok(&video)?;
    let (tx, rx) = tokio::sync::mpsc::channel::<ChatLine>(256);
    let http = state.yt.http().clone();
    tokio::spawn(async move {
        if let Err(e) = crate::ytchat::read(http, video, tx).await {
            tracing::debug!("YouTube-Chat: {e}");
        }
    });
    Ok(sse_from(rx))
}

pub async fn yt_chat_send(State(state): State<AppState>, User(me): User, Path(video): Path<String>, Json(b): Json<ChatSend>) -> R<Value> {
    video_ok(&video)?;
    let raw = state.db.account(me.id, "youtube").ok_or_else(|| bad("Zum Schreiben bitte erst unter Einstellungen → Konto deine YouTube-Anmeldung hinterlegen"))?;
    let auth = crate::ytchat::YtAuth::from_netscape(&raw).map_err(|e| bad(&e.to_string()))?;
    crate::ytchat::send(state.yt.http(), &auth, &video, &b.text).await.map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Hauptliste für VODs nur für Abonnenten (direkt aus der Ablage, über den Proxy)
pub async fn nosub(State(state): State<AppState>, User(_me): User, Path(file): Path<String>, Query(q): Query<NosubQ>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let vod = file.trim_end_matches(".m3u8");
    if vod.is_empty() || !vod.chars().all(|c| c.is_ascii_digit()) {
        return bad("Ungültige VOD-ID").into_response();
    }
    match state.twitch.nosub_master(vod).await {
        Ok(m) => crate::proxy::playlist_response(crate::proxy::rewrite_m3u8(&m, "", q.codec.as_deref(), q.max)),
        Err(e) => bad(&e.to_string()).into_response(),
    }
}

#[derive(Deserialize, Default)]
pub struct NosubQ {
    #[serde(default)]
    codec: Option<String>,
    #[serde(default)]
    max: Option<u32>,
}
