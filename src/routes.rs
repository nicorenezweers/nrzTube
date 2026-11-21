//! JSON-API unter `/api`.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use nrztube_types::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::auth::{self, Admin, User};
use crate::yt::{Locale, SearchFilter};
use crate::{db::now, import, AppState};

pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::BAD_GATEWAY, e.to_string())
    }
}

pub(crate) fn bad(msg: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.to_string())
}

pub(crate) type R<T> = Result<Json<T>, ApiError>;

fn valid_video_id(id: &str) -> bool {
    id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// YouTube-Video oder Twitch-Eintrag (tw:live:<login>, tw:vod:<id>, tw:clip:<slug>)
pub(crate) fn valid_item_id(id: &str) -> bool {
    valid_video_id(id)
        || id.len() < 120
            && is_external_id(id)
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-')
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 200 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '@' || c == '.')
}

fn loc(state: &AppState, user: &Me) -> (Settings, Locale) {
    let s = state.db.settings(user.id);
    let l = Locale::from_settings(&s);
    (s, l)
}

/// Ausblenden nach den Einstellungen (Shorts, Live, versteckte Kanäle, Stichwörter, Gesehenes)
/// Twitch-Eintrag behalten? Ausgeblendete Kanäle (tw:<login> oder Name), Wörter im Titel und
/// Kategorien – wie unwanted-twitch
pub(crate) fn keep_twitch(s: &Settings, v: &VideoItem) -> bool {
    let login = v.author_id.trim_start_matches("tw:");
    if s.hidden_channels.iter().any(|c| c == &v.author_id || c.eq_ignore_ascii_case(login) || c.eq_ignore_ascii_case(&v.author)) {
        return false;
    }
    let title = v.title.to_lowercase();
    if s.hidden_keywords.iter().any(|k| !k.trim().is_empty() && title.contains(&k.trim().to_lowercase())) {
        return false;
    }
    let game = v.description.as_deref().unwrap_or("");
    !(!game.is_empty() && s.twitch_hidden_games.iter().any(|g| g.trim().eq_ignore_ascii_case(game)))
}

fn keep_video(s: &Settings, watched: &Option<std::collections::HashSet<String>>, v: &VideoItem) -> bool {
    if s.hide_shorts && v.short || s.hide_live && v.live || s.hide_upcoming && v.upcoming {
        return false;
    }
    if !v.author_id.is_empty() && s.hidden_channels.iter().any(|c| c == &v.author_id || c.eq_ignore_ascii_case(&v.author)) {
        return false;
    }
    if !s.hidden_keywords.is_empty() {
        let title = v.title.to_lowercase();
        if s.hidden_keywords.iter().any(|k| !k.trim().is_empty() && title.contains(&k.trim().to_lowercase())) {
            return false;
        }
    }
    if let Some(w) = watched {
        if w.contains(&v.id) {
            return false;
        }
    }
    true
}

/// Darf gerade gelernt werden, was das Konto mag? (Nicht im privaten Stream-Modus)
fn records_taste(s: &Settings) -> bool {
    s.save_history && !(s.stream_enabled && s.stream_mode && s.stream_private)
}

fn watched_set(state: &AppState, s: &Settings, user: &Me) -> Option<std::collections::HashSet<String>> {
    s.hide_watched.then(|| state.db.watched_ids(user.id))
}

fn filter_videos(state: &AppState, s: &Settings, user: &Me, list: Vec<VideoItem>) -> Vec<VideoItem> {
    let w = watched_set(state, s, user);
    list.into_iter().filter(|v| keep_video(s, &w, v)).collect()
}

fn filter_items(state: &AppState, s: &Settings, user: &Me, list: Vec<Item>) -> Vec<Item> {
    let w = watched_set(state, s, user);
    list.into_iter()
        .filter(|i| match i {
            Item::Video(v) => keep_video(s, &w, v),
            Item::Channel(c) => !s.hidden_channels.iter().any(|h| h == &c.id),
            Item::Playlist(_) => !s.hide_playlists,
        })
        .collect()
}

// ------------------------------------------------------------------ Konto

pub async fn auth_status(State(state): State<AppState>, headers: HeaderMap) -> Json<AuthStatus> {
    let me = headers
        .get(header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .and_then(|c| {
            c.split(';').find_map(|p| {
                let (k, v) = p.trim().split_once('=')?;
                (k == auth::COOKIE).then(|| v.to_string())
            })
        })
        .and_then(|t| state.db.session_user(&t));
    Json(AuthStatus {
        setup: state.db.user_count() == 0,
        registration_open: state.db.app_setting("registration_open").as_deref() == Some("1"),
        me,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

fn is_secure(headers: &HeaderMap) -> bool {
    headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https")
}

fn login_response(state: &AppState, headers: &HeaderMap, id: i64) -> Result<Response, ApiError> {
    let token = auth::new_token();
    state.db.create_session(&token, id).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut resp = Json(json!({ "ok": true })).into_response();
    if let Ok(v) = auth::session_cookie(&token, is_secure(headers)).parse() {
        resp.headers_mut().insert(header::SET_COOKIE, v);
    }
    Ok(resp)
}

fn check_credentials(c: &Credentials) -> Result<(), ApiError> {
    let name = c.username.trim();
    if name.len() < 2 || name.len() > 40 || !name.chars().all(|ch| ch.is_alphanumeric() || "._-".contains(ch)) {
        return Err(bad("Benutzername: 2–40 Zeichen, Buchstaben, Ziffern, . _ -"));
    }
    if c.password.chars().count() < 8 {
        return Err(bad("Das Passwort braucht mindestens 8 Zeichen"));
    }
    Ok(())
}

pub async fn register(State(state): State<AppState>, headers: HeaderMap, Json(c): Json<Credentials>) -> Result<Response, ApiError> {
    check_credentials(&c)?;
    let first = state.db.user_count() == 0;
    let open = state.db.app_setting("registration_open").as_deref() == Some("1");
    let invited = c.invite.as_deref().is_some_and(|code| state.db.invite_valid(code));
    if !first && !open && !invited {
        return Err(ApiError(StatusCode::FORBIDDEN, if c.invite.is_some() { "Die Einladung ist ungültig, abgelaufen oder schon benutzt".into() } else { "Die Registrierung ist geschlossen".into() }));
    }
    if invited && !open && state.db.user_by_name(c.username.trim()).is_none() && !state.db.use_invite(c.invite.as_deref().unwrap_or(""), c.username.trim()) {
        return Err(ApiError(StatusCode::FORBIDDEN, "Die Einladung wurde gerade schon benutzt".into()));
    }
    let id = state
        .db
        .create_user(c.username.trim(), &auth::hash(&c.password), first)
        .map_err(|_| bad("Diesen Benutzernamen gibt es schon"))?;
    login_response(&state, &headers, id)
}

pub async fn create_invite(State(state): State<AppState>, User(me): User) -> R<Invite> {
    let code = auth::new_token()[..16].to_string();
    state.db.create_invite(me.id, &code).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(state.db.invites(me.id).into_iter().find(|i| i.code == code).unwrap_or_default()))
}

pub async fn list_invites(State(state): State<AppState>, User(me): User) -> Json<Vec<Invite>> {
    Json(state.db.invites(me.id))
}

pub async fn delete_invite(State(state): State<AppState>, User(me): User, Path(code): Path<String>) -> R<Value> {
    state.db.delete_invite(me.id, &code).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Ohne Anmeldung: ist dieser Einladungscode gültig? (für die Registrierungsseite)
pub async fn check_invite(State(state): State<AppState>, Path(code): Path<String>) -> Json<Value> {
    Json(json!({ "valid": state.db.invite_valid(&code) }))
}

pub async fn login(State(state): State<AppState>, headers: HeaderMap, Json(c): Json<Credentials>) -> Result<Response, ApiError> {
    match state.db.user_by_name(c.username.trim()) {
        Some((id, hash, _)) if auth::verify(&c.password, &hash) => login_response(&state, &headers, id),
        _ => {
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            Err(ApiError(StatusCode::UNAUTHORIZED, "Benutzername oder Passwort falsch".into()))
        }
    }
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(t) = headers.get(header::COOKIE).and_then(|c| c.to_str().ok()).and_then(|c| {
        c.split(';').find_map(|p| {
            let (k, v) = p.trim().split_once('=')?;
            (k == auth::COOKIE).then(|| v.to_string())
        })
    }) {
        state.db.delete_session(&t);
    }
    let mut resp = Json(json!({ "ok": true })).into_response();
    if let Ok(v) = auth::clear_cookie().parse() {
        resp.headers_mut().insert(header::SET_COOKIE, v);
    }
    resp
}

#[derive(Deserialize)]
pub struct PasswordChange {
    old: String,
    new: String,
}

pub async fn change_password(State(state): State<AppState>, User(me): User, Json(p): Json<PasswordChange>) -> R<Value> {
    let Some((_, hash, _)) = state.db.user_by_name(&me.username) else { return Err(bad("Konto fehlt")) };
    if !auth::verify(&p.old, &hash) {
        return Err(ApiError(StatusCode::FORBIDDEN, "Altes Passwort falsch".into()));
    }
    if p.new.chars().count() < 8 {
        return Err(bad("Das Passwort braucht mindestens 8 Zeichen"));
    }
    state.db.set_password(me.id, &auth::hash(&p.new)).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn admin_info(State(state): State<AppState>, Admin(_): Admin) -> Json<AdminInfo> {
    Json(AdminInfo {
        users: state.db.users(),
        registration_open: state.db.app_setting("registration_open").as_deref() == Some("1"),
        ytdlp_version: state.ytdlp.version.lock().await.clone(),
        youtube_cookies: state.ytdlp.has_cookies().await,
        social_cookies: state.social.status().into_iter().filter(|s| s.login).map(|s| s.platform).collect(),
    })
}

/// cookies.txt (Netscape-Format) für altersbeschränkte Videos; leerer Text löscht sie
pub async fn admin_cookies(State(state): State<AppState>, Admin(_): Admin, body: String) -> R<Value> {
    state.ytdlp.set_cookies(&body).await.map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct OpenFlag {
    open: bool,
}

pub async fn admin_registration(State(state): State<AppState>, Admin(_): Admin, Json(f): Json<OpenFlag>) -> R<Value> {
    state.db.set_app_setting("registration_open", if f.open { "1" } else { "0" }).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn admin_create_user(State(state): State<AppState>, Admin(_): Admin, Json(c): Json<Credentials>) -> R<Value> {
    check_credentials(&c)?;
    state
        .db
        .create_user(c.username.trim(), &auth::hash(&c.password), false)
        .map_err(|_| bad("Diesen Benutzernamen gibt es schon"))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn admin_delete_user(State(state): State<AppState>, Admin(me): Admin, Path(id): Path<i64>) -> R<Value> {
    if id == me.id {
        return Err(bad("Das eigene Konto lässt sich hier nicht löschen"));
    }
    state.db.delete_user(id).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn admin_update_ytdlp(State(state): State<AppState>, Admin(_): Admin) -> R<Value> {
    state.ytdlp.update().await;
    Ok(Json(json!({ "version": state.ytdlp.version.lock().await.clone() })))
}

// ------------------------------------------------------------------ Einstellungen

pub async fn get_settings(State(state): State<AppState>, User(me): User) -> Json<Settings> {
    Json(state.db.settings(me.id))
}

pub async fn put_settings(State(state): State<AppState>, User(me): User, Json(s): Json<Settings>) -> R<Settings> {
    state.db.save_settings(me.id, &s).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(s))
}

// ------------------------------------------------------------------ YouTube

#[derive(Deserialize, Default)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    sort: String,
    #[serde(default)]
    date: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    duration: String,
    #[serde(default)]
    features: String,
    #[serde(default)]
    cont: Option<String>,
}

pub async fn search(State(state): State<AppState>, User(me): User, Query(q): Query<SearchQuery>) -> R<Page<Item>> {
    if q.q.trim().is_empty() && q.cont.is_none() {
        return Ok(Json(Page { items: vec![], continuation: None }));
    }
    let (s, l) = loc(&state, &me);
    let filter = SearchFilter {
        sort: q.sort,
        date: q.date,
        kind: q.kind,
        duration: q.duration,
        features: q.features.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect(),
    };
    let page = state.yt.search(q.q.trim(), &filter, q.cont.as_deref(), &l).await?;
    if q.cont.is_none() && records_taste(&s) {
        let _ = state.db.record_search(me.id, &q.q);
    }
    Ok(Json(Page { items: filter_items(&state, &s, &me, page.items), continuation: page.continuation }))
}

#[derive(Deserialize)]
pub struct Q {
    #[serde(default)]
    q: String,
}

pub async fn suggest(State(state): State<AppState>, User(me): User, Query(q): Query<Q>) -> Json<Vec<String>> {
    let (_, l) = loc(&state, &me);
    if q.q.trim().is_empty() {
        return Json(vec![]);
    }
    Json(state.yt.suggest(&q.q, &l).await.unwrap_or_default())
}

#[derive(Deserialize)]
pub struct Cat {
    #[serde(default)]
    cat: String,
}

pub async fn trending(State(state): State<AppState>, User(me): User, Query(c): Query<Cat>) -> R<Vec<VideoItem>> {
    let (s, l) = loc(&state, &me);
    let key = format!("{}:{}:{}:{}", c.cat, l.hl, l.gl, l.safe);
    if let Some((t, v)) = state.trending.lock().await.get(&key) {
        if t.elapsed() < std::time::Duration::from_secs(1800) {
            return Ok(Json(filter_videos(&state, &s, &me, v.clone())));
        }
    }
    let list = state.yt.trending(&c.cat, &l).await?;
    state.trending.lock().await.insert(key, (std::time::Instant::now(), list.clone()));
    Ok(Json(filter_videos(&state, &s, &me, list)))
}

#[derive(Deserialize)]
pub struct ListQ {
    #[serde(default)]
    list: Option<String>,
}

pub async fn video(State(state): State<AppState>, User(me): User, Path(id): Path<String>, Query(q): Query<ListQ>) -> R<VideoDetails> {
    if !valid_video_id(&id) {
        return Err(bad("Ungültige Video-ID"));
    }
    let (s, l) = loc(&state, &me);
    // Streams schon im Hintergrund holen, damit der Player sofort loslegen kann
    let y = state.ytdlp.clone();
    let vid = id.clone();
    tokio::spawn(async move {
        let _ = y.streams(&vid).await;
    });
    let mut d = state.yt.video(&id, q.list.as_deref().filter(|x| valid_id(x)), &l).await?;
    if s.family_friendly && !d.family_safe {
        return Err(ApiError(StatusCode::FORBIDDEN, "Dieses Video ist nicht jugendfrei und wird wegen der Einstellung „Nur familienfreundliche Inhalte“ nicht gezeigt.".into()));
    }
    d.related = filter_videos(&state, &s, &me, std::mem::take(&mut d.related));
    // Vorrat für den nächsten Klick: Details der ersten ähnlichen Videos vorladen,
    // beim allerersten auch schon die Streams
    for (i, r) in d.related.iter().filter(|r| valid_video_id(&r.id) && !r.live).take(4).enumerate() {
        state.yt.warm(&r.id, &l);
        if i == 0 {
            state.ytdlp.prefetch(&r.id);
        }
    }
    Ok(Json(d))
}

pub async fn streams(State(state): State<AppState>, User(me): User, Path(id): Path<String>) -> R<StreamInfo> {
    if !valid_video_id(&id) {
        return Err(bad("Ungültige Video-ID"));
    }
    let s = state.db.settings(me.id);
    let info = state.ytdlp.streams(&id).await?;
    if s.family_friendly && info.age_limit.unwrap_or(0) >= 18 {
        return Err(ApiError(StatusCode::FORBIDDEN, "Nicht jugendfrei".into()));
    }
    Ok(Json((*info).clone()))
}

pub async fn external(State(state): State<AppState>, User(_me): User, Path(id): Path<String>, headers: HeaderMap) -> R<ExternalLinks> {
    if !valid_video_id(&id) {
        return Err(bad("Ungültige Video-ID"));
    }
    let info = state.ytdlp.streams(&id).await?;
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    let scheme = if is_secure(&headers) { "https" } else { "http" };
    let abs = |p: &str| format!("{scheme}://{host}{p}");
    let progressive = info.formats.iter().filter(|f| f.kind == "av").max_by_key(|f| f.height).map(|f| abs(&f.url));
    let audio = info.formats.iter().filter(|f| f.kind == "audio" && f.ext == "m4a").max_by_key(|f| f.bitrate).map(|f| abs(&f.url));
    Ok(Json(ExternalLinks { hls: info.hls.as_deref().map(|h| abs(&format!("{h}.m3u8"))), progressive, audio, title: String::new() }))
}

#[derive(Deserialize)]
pub struct TokenQ {
    token: String,
}

pub async fn comments(State(state): State<AppState>, User(me): User, Query(q): Query<TokenQ>) -> R<CommentPage> {
    let (_, l) = loc(&state, &me);
    Ok(Json(state.yt.comments(&q.token, &l).await?))
}

pub async fn channel(State(state): State<AppState>, User(me): User, Path(id): Path<String>) -> R<Channel> {
    if !valid_id(&id) {
        return Err(bad("Ungültige Kanal-ID"));
    }
    let (_, l) = loc(&state, &me);
    let cid = state.yt.channel_id(&id, &l).await?;
    let ch = state.yt.channel(&cid, &l).await?;
    Ok(Json(ch))
}

#[derive(Deserialize)]
pub struct ContQ {
    #[serde(default)]
    cont: Option<String>,
}

#[derive(serde::Serialize)]
pub struct TabPage {
    items: Vec<Item>,
    posts: Vec<Post>,
    continuation: Option<String>,
}

pub async fn channel_tab(
    State(state): State<AppState>,
    User(me): User,
    Path((id, tab)): Path<(String, String)>,
    Query(q): Query<ContQ>,
) -> R<TabPage> {
    if !valid_id(&id) {
        return Err(bad("Ungültige Kanal-ID"));
    }
    let (s, l) = loc(&state, &me);
    let (page, posts) = state.yt.channel_tab(&id, &tab, q.cont.as_deref(), &l).await?;
    let mut items = page.items;
    // Im Kanal selbst nicht nach Kanal filtern, nur nach Art und Gesehenem
    let w = watched_set(&state, &s, &me);
    items.retain(|i| match i {
        Item::Video(v) => {
            !(s.hide_live && v.live || s.hide_upcoming && v.upcoming || s.hide_shorts && v.short && tab != "shorts")
                && w.as_ref().map(|w| !w.contains(&v.id)).unwrap_or(true)
        }
        _ => true,
    });
    Ok(Json(TabPage { items, posts, continuation: page.continuation }))
}

pub async fn channel_search(State(state): State<AppState>, User(me): User, Path(id): Path<String>, Query(q): Query<Q>) -> R<Page<Item>> {
    let (_, l) = loc(&state, &me);
    Ok(Json(state.yt.channel_search(&id, &q.q, &l).await?))
}

pub async fn playlist(State(state): State<AppState>, User(me): User, Path(id): Path<String>, Query(q): Query<ContQ>) -> R<PlaylistDetails> {
    if !valid_id(&id) {
        return Err(bad("Ungültige Playlist-ID"));
    }
    let (_, l) = loc(&state, &me);
    Ok(Json(state.yt.playlist(&id, q.cont.as_deref(), &l).await?))
}

/// Eingefügte YouTube-Adressen erkennen: Video, Kanal, Playlist
pub async fn resolve(State(state): State<AppState>, User(me): User, Query(q): Query<Q>) -> R<Value> {
    let input = q.q.trim();
    let (_, l) = loc(&state, &me);
    let lower = input.to_lowercase();
    let is_url = lower.contains("youtube.com/") || lower.contains("youtu.be/") || lower.starts_with('@');
    if !is_url {
        return Ok(Json(json!({ "kind": "none" })));
    }
    let get = |k: &str| -> Option<String> {
        let i = input.find(&format!("{k}="))? + k.len() + 1;
        Some(input[i..].split(['&', '#']).next()?.to_string())
    };
    if let Some(v) = get("v").filter(|v| valid_video_id(v)) {
        return Ok(Json(json!({ "kind": "video", "id": v, "list": get("list"), "t": get("t") })));
    }
    for pre in ["youtu.be/", "/shorts/", "/live/", "/embed/"] {
        if let Some(i) = input.find(pre) {
            let id: String = input[i + pre.len()..].chars().take(11).collect();
            if valid_video_id(&id) {
                return Ok(Json(json!({ "kind": "video", "id": id, "t": get("t") })));
            }
        }
    }
    if let Some(list) = get("list") {
        return Ok(Json(json!({ "kind": "playlist", "id": list })));
    }
    let url = if input.starts_with('@') { format!("https://www.youtube.com/{input}") } else { input.to_string() };
    let ep = state.yt.resolve(&url, &l).await?;
    if let Some(b) = ep.pointer("/browseEndpoint/browseId").and_then(Value::as_str) {
        if let Some(pl) = b.strip_prefix("VL") {
            return Ok(Json(json!({ "kind": "playlist", "id": pl })));
        }
        return Ok(Json(json!({ "kind": "channel", "id": b })));
    }
    if let Some(v) = ep.pointer("/watchEndpoint/videoId").and_then(Value::as_str) {
        return Ok(Json(json!({ "kind": "video", "id": v })));
    }
    Ok(Json(json!({ "kind": "none" })))
}

pub async fn sponsorblock(State(state): State<AppState>, User(_me): User, Path(id): Path<String>) -> R<Vec<Segment>> {
    if !valid_video_id(&id) {
        return Err(bad("Ungültige Video-ID"));
    }
    Ok(Json(state.sb.segments(&id).await))
}

#[derive(Deserialize)]
pub struct IdsQ {
    ids: String,
}

#[derive(serde::Serialize)]
pub struct DeArrow {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumb: Option<String>,
}

pub async fn dearrow(State(state): State<AppState>, User(me): User, Query(q): Query<IdsQ>) -> Json<HashMap<String, DeArrow>> {
    let s = state.db.settings(me.id);
    if !s.dearrow_titles && !s.dearrow_thumbs {
        return Json(HashMap::new());
    }
    let ids: Vec<String> = q.ids.split(',').filter(|x| valid_video_id(x)).take(100).map(str::to_string).collect();
    let sb = state.sb.clone();
    let results: Vec<(String, crate::sb::Branding)> = futures::stream::iter(ids)
        .map(|id| {
            let sb = sb.clone();
            async move {
                let b = sb.branding(&id).await;
                (id, b)
            }
        })
        .buffer_unordered(12)
        .collect()
        .await;
    let mut out = HashMap::new();
    for (id, b) in results {
        let title = if s.dearrow_titles { b.title } else { None };
        let thumb = if s.dearrow_thumbs { b.thumb_time.map(|t| crate::sb::dearrow_thumb(&id, t)) } else { None };
        if title.is_some() || thumb.is_some() {
            out.insert(id, DeArrow { title, thumb });
        }
    }
    Json(out)
}

// ------------------------------------------------------------------ Abos und Feed

#[derive(Deserialize)]
pub struct FeedQ {
    #[serde(default)]
    profile: Option<i64>,
    #[serde(default)]
    force: bool,
}

pub async fn feed(State(state): State<AppState>, User(me): User, Query(q): Query<FeedQ>) -> Json<Vec<VideoItem>> {
    let (s, _) = loc(&state, &me);
    let subs = state.db.subscriptions(me.id);
    let mut ids: Vec<String> = subs.iter().map(|s| s.channel_id.clone()).collect();
    if let Some(pid) = q.profile.filter(|p| *p > 0) {
        if let Some(p) = state.db.profiles(me.id).into_iter().find(|p| p.id == pid) {
            ids.retain(|c| p.channels.contains(c));
        }
    }
    let list = state.feeds.many(&ids, q.force).await;
    // Avatare aus den Abos ergänzen
    let thumbs: HashMap<&str, &str> = subs.iter().map(|s| (s.channel_id.as_str(), s.thumb.as_str())).collect();
    let mut list = filter_videos(&state, &s, &me, list);
    for v in list.iter_mut() {
        if let Some(t) = thumbs.get(v.author_id.as_str()).filter(|t| !t.is_empty()) {
            v.author_thumb = Some((*t).to_string());
        }
    }
    list.truncate(300);
    Json(list)
}

pub async fn subscriptions(State(state): State<AppState>, User(me): User) -> Json<Vec<Subscription>> {
    Json(state.db.subscriptions(me.id))
}

pub async fn subscribe(State(state): State<AppState>, User(me): User, Json(s): Json<Subscription>) -> R<Value> {
    if !(s.channel_id.starts_with("UC") && s.channel_id.len() == 24) {
        return Err(bad("Ungültige Kanal-ID"));
    }
    state.db.subscribe(me.id, &s).map_err(|e| bad(&e.to_string()))?;
    let feeds = state.feeds.clone();
    let id = s.channel_id.clone();
    tokio::spawn(async move {
        feeds.channel(&id, false).await;
    });
    Ok(Json(json!({ "ok": true })))
}

pub async fn unsubscribe(State(state): State<AppState>, User(me): User, Path(id): Path<String>) -> R<Value> {
    state.db.unsubscribe(me.id, &id).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn is_subscribed(State(state): State<AppState>, User(me): User, Path(id): Path<String>) -> Json<bool> {
    Json(state.db.is_subscribed(me.id, &id))
}

pub async fn profiles(State(state): State<AppState>, User(me): User) -> Json<Vec<Profile>> {
    Json(state.db.profiles(me.id))
}

pub async fn save_profile(State(state): State<AppState>, User(me): User, Json(p): Json<Profile>) -> R<Value> {
    if p.name.trim().is_empty() {
        return Err(bad("Name fehlt"));
    }
    let id = state.db.save_profile(me.id, &p).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "id": id })))
}

pub async fn delete_profile(State(state): State<AppState>, User(me): User, Path(id): Path<i64>) -> R<Value> {
    state.db.delete_profile(me.id, id).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ Playlists

pub async fn playlists(State(state): State<AppState>, User(me): User) -> Json<Vec<LocalPlaylist>> {
    // Ältere Konten bekommen die Stream-Liste beim ersten Abruf
    let _ = state.db.special_playlist(me.id, crate::db::STREAM_LIST);
    let _ = state.db.special_playlist(me.id, crate::db::FAVORITES);
    Json(state.db.playlists(me.id))
}

pub async fn local_playlist(State(state): State<AppState>, User(me): User, Path(id): Path<i64>) -> R<LocalPlaylist> {
    state.db.playlist(me.id, id).map(Json).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Playlist nicht gefunden".into()))
}

#[derive(Deserialize)]
pub struct PlaylistBody {
    title: String,
    #[serde(default)]
    description: String,
}

pub async fn create_playlist(State(state): State<AppState>, User(me): User, Json(b): Json<PlaylistBody>) -> R<Value> {
    if b.title.trim().is_empty() {
        return Err(bad("Titel fehlt"));
    }
    let id = state.db.create_playlist(me.id, b.title.trim(), &b.description).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_playlist(State(state): State<AppState>, User(me): User, Path(id): Path<i64>, Json(b): Json<PlaylistBody>) -> R<Value> {
    state.db.update_playlist(me.id, id, b.title.trim(), &b.description).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_playlist(State(state): State<AppState>, User(me): User, Path(id): Path<i64>) -> R<Value> {
    state.db.delete_playlist(me.id, id).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn add_to_playlist(State(state): State<AppState>, User(me): User, Path(id): Path<i64>, Json(v): Json<VideoItem>) -> R<Value> {
    if !valid_item_id(&v.id) {
        return Err(bad("Ungültige Video-ID"));
    }
    let added = state.db.add_to_playlist(me.id, id, &v).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "added": added })))
}

pub async fn remove_from_playlist(State(state): State<AppState>, User(me): User, Path((id, vid)): Path<(i64, String)>) -> R<Value> {
    state.db.remove_from_playlist(me.id, id, &vid).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct MoveBody {
    video: String,
    delta: i64,
}

pub async fn move_in_playlist(State(state): State<AppState>, User(me): User, Path(id): Path<i64>, Json(b): Json<MoveBody>) -> R<Value> {
    state.db.move_in_playlist(me.id, id, &b.video, b.delta).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn playlists_with(State(state): State<AppState>, User(me): User, Path(vid): Path<String>) -> Json<Vec<i64>> {
    Json(state.db.playlists_with(me.id, &vid))
}

// ------------------------------------------------------------------ Verlauf

#[derive(Deserialize)]
pub struct HistQ {
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: i64,
}

pub async fn history(State(state): State<AppState>, User(me): User, Query(q): Query<HistQ>) -> Json<Vec<HistoryEntry>> {
    Json(state.db.history(me.id, q.q.trim(), 60, q.offset.max(0)))
}

pub async fn history_entry(State(state): State<AppState>, User(me): User, Path(vid): Path<String>) -> Json<Option<HistoryEntry>> {
    Json(state.db.history_entry(me.id, &vid))
}

pub async fn add_history(State(state): State<AppState>, User(me): User, Json(h): Json<HistoryUpdate>) -> R<Value> {
    let s = state.db.settings(me.id);
    if !records_taste(&s) || !valid_item_id(&h.video.id) {
        return Ok(Json(json!({ "ok": false })));
    }
    let progress = if s.save_position && !h.video.id.starts_with("tw:live:") { h.progress } else { 0.0 };
    state
        .db
        .record_watch(me.id, &h.video, progress, None, h.played, h.duration, h.opened)
        .map_err(|e| bad(&e.to_string()))?;
    state.reco.invalidate_soon(me.id).await;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ Bewertungen

pub async fn get_rating(State(state): State<AppState>, User(me): User, Path(id): Path<String>) -> R<Rating> {
    if !valid_item_id(&id) {
        return Err(bad("Ungültige Video-ID"));
    }
    let s = state.db.settings(me.id);
    let mut r = Rating { rating: state.db.rating(me.id, &id), ..Default::default() };
    if s.ryd && !is_external_id(&id) {
        if let Some((likes, dislikes)) = state.reco.ryd(&id).await {
            r.likes = Some(likes);
            r.dislikes = Some(dislikes);
        }
    }
    Ok(Json(r))
}

pub async fn rate(State(state): State<AppState>, User(me): User, Path(id): Path<String>, Json(b): Json<RateBody>) -> R<Value> {
    if !valid_item_id(&id) || b.video.id != id {
        return Err(bad("Ungültige Video-ID"));
    }
    state.db.rate(me.id, &b.video, b.rating.clamp(-1, 1)).map_err(|e| bad(&e.to_string()))?;
    state.reco.invalidate(me.id).await;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ Startseite

#[derive(Deserialize)]
pub struct HomeQ {
    #[serde(default)]
    refresh: bool,
}

pub async fn home(State(state): State<AppState>, User(me): User, Query(q): Query<HomeQ>) -> Json<HomeFeed> {
    let (s, l) = loc(&state, &me);
    let mut feed = crate::reco::home(&state, &me, &l, q.refresh).await;
    let w = watched_set(&state, &s, &me);
    feed.items.retain(|i| keep_video(&s, &w, &i.video));
    Json(feed)
}

pub async fn taste(State(state): State<AppState>, User(me): User) -> Json<Taste> {
    Json(crate::reco::taste(&state, me.id))
}

pub async fn reset_taste(State(state): State<AppState>, User(me): User) -> R<Value> {
    state.db.reset_taste(me.id).map_err(|e| bad(&e.to_string()))?;
    state.reco.invalidate(me.id).await;
    Ok(Json(json!({ "ok": true })))
}

/// Streams im Hintergrund vorladen (Maus über einem Video, nächstes Video)
pub async fn prefetch(State(state): State<AppState>, User(me): User, Json(b): Json<IdsBody>) -> Json<Value> {
    let (_, l) = loc(&state, &me);
    for id in b.ids.iter().filter(|i| valid_video_id(i)).take(3) {
        state.ytdlp.prefetch(id);
        // Titel, Beschreibung und ähnliche Videos gleich mit
        state.yt.warm(id, &l);
    }
    Json(json!({ "ok": true }))
}

pub async fn delete_history_entry(State(state): State<AppState>, User(me): User, Path(vid): Path<String>) -> R<Value> {
    state.db.delete_history(me.id, Some(&vid)).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn clear_history(State(state): State<AppState>, User(me): User) -> R<Value> {
    state.db.delete_history(me.id, None).map_err(|e| bad(&e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ Export und Import

#[derive(Deserialize)]
pub struct FormatQ {
    #[serde(default)]
    format: String,
}

fn download(body: String, ct: &str, name: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, ct.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        body,
    )
        .into_response()
}

pub async fn export_subscriptions(State(state): State<AppState>, User(me): User, Query(f): Query<FormatQ>) -> Response {
    let (body, ct, name) = import::export_subscriptions(&state.db, me.id, &f.format);
    download(body, ct, &name)
}

pub async fn import_subscriptions(State(state): State<AppState>, User(me): User, body: Bytes) -> R<ImportResult> {
    let text = String::from_utf8_lossy(&body);
    let parsed = import::parse_subscriptions(&text);
    let mut r = ImportResult { format: parsed.format.clone(), ..Default::default() };
    let existing: std::collections::HashSet<String> = state.db.subscriptions(me.id).into_iter().map(|s| s.channel_id).collect();
    let mut missing_names = Vec::new();
    for s in &parsed.subs {
        if existing.contains(&s.channel_id) {
            r.skipped += 1;
            continue;
        }
        if state.db.subscribe(me.id, s).is_ok() {
            r.added += 1;
            if s.name.is_empty() {
                missing_names.push(s.channel_id.clone());
            }
        }
    }
    // Profile aus FreeTube übernehmen (gleichnamige ergänzen)
    let profiles = state.db.profiles(me.id);
    for (name, color, chans) in parsed.profiles {
        let mut p = profiles.iter().find(|p| p.name == name).cloned().unwrap_or(Profile { id: 0, name, color, channels: vec![] });
        for c in chans {
            if !p.channels.contains(&c) {
                p.channels.push(c);
            }
        }
        let _ = state.db.save_profile(me.id, &p);
    }
    // Namen fehlender Kanäle (Invidious-Export) aus den RSS-Feeds nachtragen
    if !missing_names.is_empty() {
        let st = state.clone();
        let uid = me.id;
        tokio::spawn(async move {
            for id in missing_names {
                let url = format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}");
                if let Ok(r) = st.yt.http().get(&url).send().await {
                    if let Ok(t) = r.text().await {
                        let (name, _) = crate::feed::parse_rss(&t);
                        if !name.is_empty() {
                            let _ = st.db.subscribe(uid, &Subscription { channel_id: id, name, thumb: String::new(), added: now() });
                        }
                    }
                }
            }
        });
    }
    Ok(Json(r))
}

pub async fn export_playlists(State(state): State<AppState>, User(me): User) -> Response {
    let date = chrono::Utc::now().format("%Y-%m-%d");
    download(import::export_playlists(&state.db, me.id), "application/x-ndjson", &format!("nrztube-freetube-playlists-{date}.db"))
}

pub async fn import_playlists(State(state): State<AppState>, User(me): User, body: Bytes) -> Json<ImportResult> {
    Json(import::import_playlists(&state.db, me.id, &String::from_utf8_lossy(&body)))
}

pub async fn export_history(State(state): State<AppState>, User(me): User) -> Response {
    let date = chrono::Utc::now().format("%Y-%m-%d");
    download(import::export_history(&state.db, me.id), "application/x-ndjson", &format!("nrztube-freetube-history-{date}.db"))
}

pub async fn import_history(State(state): State<AppState>, User(me): User, body: Bytes) -> Json<ImportResult> {
    Json(import::import_history(&state.db, me.id, &String::from_utf8_lossy(&body)))
}

/// Abo-Avatare nachladen (für importierte Abos ohne Bild)
pub async fn refresh_subscription_thumbs(State(state): State<AppState>, User(me): User) -> Json<Value> {
    let (_, l) = loc(&state, &me);
    let subs: Vec<Subscription> = state.db.subscriptions(me.id).into_iter().filter(|s| s.thumb.is_empty() || s.name.is_empty()).collect();
    let count = subs.len();
    let st = state.clone();
    tokio::spawn(async move {
        for s in subs {
            if let Ok(ch) = st.yt.channel(&s.channel_id, &l).await {
                let _ = st.db.subscribe(me.id, &Subscription { channel_id: s.channel_id, name: ch.name, thumb: ch.avatar, added: s.added });
            }
        }
    });
    Json(json!({ "pending": count }))
}
