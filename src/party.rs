//! Watch-Party: zusammen schauen. Räume leben im Speicher (verschwinden nach 12 h ohne
//! Teilnehmer oder beim Neustart). Mitglieder sind Konten oder Gäste mit Einladungslink;
//! Gäste bekommen ein Cookie, das nur für diesen Raum gilt.

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use nrztube_types::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex};

use crate::auth::{self, User};
use crate::routes::{bad, valid_item_id, ApiError, R};
use crate::AppState;

pub const GUEST_COOKIE: &str = "nrztube_party";
const IDLE: Duration = Duration::from_secs(12 * 3600);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

struct Member {
    name: String,
    user: Option<i64>,
    /// Geheimer Gast-Schlüssel (im Cookie)
    guest_token: Option<String>,
    /// Offene Ereignis-Verbindungen
    online: u32,
    perms: PartyPerms,
}

struct RoomState {
    title: String,
    video: Option<VideoItem>,
    playing: bool,
    position: f64,
    at: i64,
    queue: Vec<VideoItem>,
    members: HashMap<String, Member>,
    host: String,
    public: bool,
    allow_guests: bool,
    default_perms: PartyPerms,
    by: String,
    chat: VecDeque<PartyChatMsg>,
    last_active: Instant,
}

pub struct Room {
    id: String,
    invite: String,
    st: Mutex<RoomState>,
    tx: broadcast::Sender<PartyEvent>,
}

#[derive(Default)]
pub struct Parties {
    rooms: Mutex<HashMap<String, Arc<Room>>>,
}

impl RoomState {
    /// Aktuelle Position (läuft das Video, seit `at` weitergerechnet)
    fn pos_now(&self) -> f64 {
        if self.playing { self.position + (now_ms() - self.at) as f64 / 1000.0 } else { self.position }
    }

    fn snapshot(&self, id: &str) -> PartyState {
        let mut members: Vec<PartyMember> = self
            .members
            .iter()
            .map(|(mid, m)| PartyMember {
                id: mid.clone(),
                name: m.name.clone(),
                guest: m.user.is_none(),
                host: *mid == self.host,
                online: m.online > 0,
                perms: if *mid == self.host { PartyPerms::ALL } else { m.perms },
            })
            .collect();
        members.sort_by(|a, b| b.host.cmp(&a.host).then(b.online.cmp(&a.online)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        PartyState {
            id: id.to_string(),
            title: self.title.clone(),
            video: self.video.clone(),
            playing: self.playing,
            position: self.position,
            at: self.at,
            now: now_ms(),
            queue: self.queue.clone(),
            members,
            public: self.public,
            allow_guests: self.allow_guests,
            default_perms: self.default_perms,
            by: self.by.clone(),
            invite: String::new(),
            me: String::new(),
        }
    }
}

impl Room {
    async fn broadcast_state(&self) {
        let s = self.st.lock().await.snapshot(&self.id);
        let _ = self.tx.send(PartyEvent::State { s });
    }

    async fn system(&self, text: String) {
        let m = PartyChatMsg { name: String::new(), text, at: now_ms(), system: true };
        let mut st = self.st.lock().await;
        st.chat.push_back(m.clone());
        if st.chat.len() > 100 {
            st.chat.pop_front();
        }
        drop(st);
        let _ = self.tx.send(PartyEvent::Chat { m });
    }
}

impl Parties {
    async fn get(&self, id: &str) -> Option<Arc<Room>> {
        let mut rooms = self.rooms.lock().await;
        // Aufräumen: lange niemand mehr da
        let mut dead = vec![];
        for (k, r) in rooms.iter() {
            if let Ok(st) = r.st.try_lock() {
                if st.last_active.elapsed() > IDLE && st.members.values().all(|m| m.online == 0) {
                    dead.push(k.clone());
                }
            }
        }
        for k in dead {
            rooms.remove(&k);
        }
        rooms.get(id).cloned()
    }

    /// Gültiges Gast-Cookie (für Bilder über den Proxy)
    pub async fn valid_guest(&self, cookie: &str) -> bool {
        let Some((room, tok)) = cookie.split_once('.') else { return false };
        let Some(r) = self.get(room).await else { return false };
        let st = r.st.lock().await;
        st.members.values().any(|m| m.guest_token.as_deref() == Some(tok))
    }
}

pub fn guest_cookie_from(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|c| {
        let (k, v) = c.trim().split_once('=')?;
        (k == GUEST_COOKIE).then(|| v.to_string())
    })
}

/// Angemeldetes Konto oder Gast – optional
pub struct Viewer {
    pub user: Option<Me>,
    pub guest: Option<String>,
}

impl FromRequestParts<AppState> for Viewer {
    type Rejection = Response;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let user = auth::token_from(parts).and_then(|t| state.db.session_user(&t));
        Ok(Viewer { user, guest: guest_cookie_from(&parts.headers) })
    }
}

/// Mitglieds-ID im Raum (oder 403)
async fn member_id(room: &Room, v: &Viewer) -> Result<String, ApiError> {
    let st = room.st.lock().await;
    if let Some(me) = &v.user {
        let id = format!("u{}", me.id);
        if st.members.contains_key(&id) {
            return Ok(id);
        }
    }
    if let Some(c) = &v.guest {
        if let Some((rid, tok)) = c.split_once('.') {
            if rid == room.id {
                if let Some((mid, _)) = st.members.iter().find(|(_, m)| m.guest_token.as_deref() == Some(tok)) {
                    return Ok(mid.clone());
                }
            }
        }
    }
    Err(ApiError(StatusCode::FORBIDDEN, "Du bist nicht in dieser Watch-Party – bitte über den Einladungslink beitreten".into()))
}

fn room_ok(id: &str) -> Result<(), ApiError> {
    if id.len() == 10 && id.chars().all(|c| c.is_ascii_alphanumeric()) { Ok(()) } else { Err(bad("Ungültiger Raum")) }
}

fn short_id(n: usize) -> String {
    const A: &[u8] = b"abcdefghijkmnopqrstuvwxyz23456789";
    (0..n).map(|_| A[rand::random::<usize>() % A.len()] as char).collect()
}

// ------------------------------------------------------------------ Routen

pub async fn create(State(state): State<AppState>, User(me): User, Json(b): Json<PartyCreate>) -> R<Value> {
    let id = short_id(10);
    let invite = auth::new_token()[..24].to_string();
    let host = format!("u{}", me.id);
    let video = b.video.filter(|v| valid_item_id(&v.id));
    let mut members = HashMap::new();
    members.insert(host.clone(), Member { name: me.username.clone(), user: Some(me.id), guest_token: None, online: 0, perms: PartyPerms::ALL });
    let title = if b.title.trim().is_empty() { format!("Watch-Party von {}", me.username) } else { b.title.trim().chars().take(80).collect() };
    let (tx, _) = broadcast::channel(256);
    let room = Arc::new(Room {
        id: id.clone(),
        invite: invite.clone(),
        st: Mutex::new(RoomState {
            title,
            video,
            playing: false,
            position: 0.0,
            at: now_ms(),
            queue: vec![],
            members,
            host,
            public: b.public,
            allow_guests: true,
            default_perms: PartyPerms::default(),
            by: String::new(),
            chat: VecDeque::new(),
            last_active: Instant::now(),
        }),
        tx,
    });
    state.parties.rooms.lock().await.insert(id.clone(), room);
    Ok(Json(json!({ "id": id, "invite": invite })))
}

#[derive(Deserialize, Default)]
pub struct KeyQ {
    #[serde(default)]
    k: String,
}

/// Für die Beitrittsseite: nur mit gültigem Schlüssel oder als Mitglied
pub async fn info(State(state): State<AppState>, v: Viewer, Path(id): Path<String>, Query(q): Query<KeyQ>) -> R<PartyInfo> {
    room_ok(&id)?;
    let room = state.parties.get(&id).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Diese Watch-Party gibt es nicht mehr".into()))?;
    let joined = member_id(&room, &v).await.is_ok();
    let public = room.st.lock().await.public;
    if !joined && !public && q.k != room.invite {
        return Err(ApiError(StatusCode::FORBIDDEN, "Einladungslink ungültig".into()));
    }
    let st = room.st.lock().await;
    Ok(Json(info_of(&room, &st, joined)))
}

pub async fn join(State(state): State<AppState>, v: Viewer, headers: HeaderMap, Path(id): Path<String>, Json(b): Json<PartyJoin>) -> Result<Response, ApiError> {
    room_ok(&id)?;
    let room = state.parties.get(&id).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Diese Watch-Party gibt es nicht mehr".into()))?;
    if member_id(&room, &v).await.is_ok() {
        return Ok(Json(json!({ "ok": true })).into_response());
    }
    let (public, guests_ok) = {
        let st = room.st.lock().await;
        (st.public, st.allow_guests)
    };
    if b.k != room.invite && !public {
        return Err(ApiError(StatusCode::FORBIDDEN, "Einladungslink ungültig".into()));
    }
    if v.user.is_none() && !guests_ok {
        return Err(ApiError(StatusCode::FORBIDDEN, "In dieser Watch-Party sind nur Gäste mit nrzTube-Konto erlaubt – bitte anmelden".into()));
    }
    let mut cookie = None;
    let name;
    {
        let mut st = room.st.lock().await;
        if let Some(me) = &v.user {
            name = me.username.clone();
            let perms = st.default_perms;
            st.members.insert(format!("u{}", me.id), Member { name: name.clone(), user: Some(me.id), guest_token: None, online: 0, perms });
        } else {
            let n: String = b.name.trim().chars().filter(|c| !c.is_control()).take(24).collect();
            if n.is_empty() {
                return Err(bad("Bitte einen Namen eingeben"));
            }
            // Gleiche Namen unterscheidbar machen
            let taken = st.members.values().any(|m| m.name.eq_ignore_ascii_case(&n));
            name = if taken { format!("{n} ({})", st.members.len() + 1) } else { n };
            let tok = auth::new_token();
            let perms = st.default_perms;
            st.members.insert(format!("g{}", short_id(8)), Member { name: name.clone(), user: None, guest_token: Some(tok.clone()), online: 0, perms });
            let secure = headers.get("x-forwarded-proto").and_then(|h| h.to_str().ok()) == Some("https");
            cookie = Some(format!("{GUEST_COOKIE}={id}.{tok}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}", 86400, if secure { "; Secure" } else { "" }));
        }
        st.last_active = Instant::now();
    }
    room.system(format!("{name} ist beigetreten")).await;
    room.broadcast_state().await;
    let mut resp = Json(json!({ "ok": true })).into_response();
    if let Some(c) = cookie {
        if let Ok(h) = c.parse() {
            resp.headers_mut().insert(header::SET_COOKIE, h);
        }
    }
    Ok(resp)
}

/// Zustand und Chat als Server-Sent Events (zählt auch, wer online ist)
pub async fn events(State(state): State<AppState>, v: Viewer, Path(id): Path<String>) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    room_ok(&id)?;
    let room = state.parties.get(&id).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Diese Watch-Party gibt es nicht mehr".into()))?;
    let mid = member_id(&room, &v).await?;
    let rx = room.tx.subscribe();
    // Erst: eigener Zustand (mit Einladung und eigener ID) und die letzten Chat-Nachrichten
    let (first, history) = {
        let mut st = room.st.lock().await;
        if let Some(m) = st.members.get_mut(&mid) {
            m.online += 1;
        }
        st.last_active = Instant::now();
        let mut s = st.snapshot(&id);
        s.invite = room.invite.clone();
        s.me = mid.clone();
        (s, st.chat.iter().cloned().collect::<Vec<_>>())
    };
    room.broadcast_state().await;
    struct Guard {
        room: Arc<Room>,
        mid: String,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            let room = self.room.clone();
            let mid = self.mid.clone();
            tokio::spawn(async move {
                {
                    let mut st = room.st.lock().await;
                    if let Some(m) = st.members.get_mut(&mid) {
                        m.online = m.online.saturating_sub(1);
                    }
                    st.last_active = Instant::now();
                }
                room.broadcast_state().await;
            });
        }
    }
    let guard = Guard { room: room.clone(), mid: mid.clone() };
    let mut initial: VecDeque<PartyEvent> = VecDeque::new();
    initial.push_back(PartyEvent::State { s: first });
    for m in history {
        initial.push_back(PartyEvent::Chat { m });
    }
    let me = mid.clone();
    let invite = room.invite.clone();
    let stream = futures::stream::unfold((initial, rx, guard), move |(mut initial, mut rx, guard)| {
        let me = me.clone();
        let invite = invite.clone();
        async move {
            let ev = match initial.pop_front() {
                Some(e) => e,
                None => loop {
                    match rx.recv().await {
                        Ok(e) => break e,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => return None,
                    }
                },
            };
            // Eigene ID und Einladung in jeden Zustand
            let ev = match ev {
                PartyEvent::State { mut s } => {
                    s.me = me.clone();
                    s.invite = invite.clone();
                    PartyEvent::State { s }
                }
                e => e,
            };
            let closed = matches!(ev, PartyEvent::Closed);
            let out = Event::default().json_data(&ev).unwrap_or_else(|_| Event::default());
            if closed {
                // Letztes Ereignis senden, dann Schluss
                return Some((Ok(out), (VecDeque::new(), rx, guard)));
            }
            Some((Ok(out), (initial, rx, guard)))
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

pub async fn action(State(state): State<AppState>, v: Viewer, Path(id): Path<String>, Json(a): Json<PartyAction>) -> R<Value> {
    room_ok(&id)?;
    let room = state.parties.get(&id).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Diese Watch-Party gibt es nicht mehr".into()))?;
    let mid = member_id(&room, &v).await?;
    let mut note: Option<String> = None;
    {
        let mut st = room.st.lock().await;
        st.last_active = Instant::now();
        let is_host = st.host == mid;
        let perms = if is_host { PartyPerms::ALL } else { st.members.get(&mid).map(|m| m.perms).unwrap_or_default() };
        let can = perms.control;
        let deny = |what: &str| Err(ApiError(StatusCode::FORBIDDEN, format!("Dafür fehlt dir das Recht „{what}“ – der Gastgeber kann es freigeben")));
        let name = st.members.get(&mid).map(|m| m.name.clone()).unwrap_or_default();
        let pos = if a.position.is_finite() { a.position.max(0.0) } else { 0.0 };
        match a.kind.as_str() {
            "chat" if !perms.chat => return deny("Chatten"),
            "queue" if !perms.queue => return deny("Einreihen"),
            "remove" | "playnow" if !perms.manage && !(a.kind == "playnow" && perms.control && perms.manage) => return deny("Warteschlange verwalten"),
            "chat" => {
                let text: String = a.text.replace(['\r', '\n'], " ").trim().chars().take(500).collect();
                if text.is_empty() {
                    return Ok(Json(json!({ "ok": true })));
                }
                let m = PartyChatMsg { name, text, at: now_ms(), system: false };
                st.chat.push_back(m.clone());
                if st.chat.len() > 100 {
                    st.chat.pop_front();
                }
                drop(st);
                let _ = room.tx.send(PartyEvent::Chat { m });
                return Ok(Json(json!({ "ok": true })));
            }
            "queue" => {
                let vid = a.video.clone().filter(|v| valid_item_id(&v.id)).ok_or_else(|| bad("Ungültiges Video"))?;
                if st.video.is_none() {
                    st.video = Some(vid.clone());
                    st.position = 0.0;
                    st.playing = true;
                    st.at = now_ms();
                } else if st.queue.len() < 200 {
                    st.queue.push(vid.clone());
                }
                note = Some(format!("{name} hat „{}“ eingereiht", vid.title));
            }
            "play" | "pause" | "seek" | "load" | "next" if !can => return deny("Steuern"),
            "play" => {
                st.position = pos;
                st.playing = true;
                st.at = now_ms();
            }
            "pause" => {
                st.position = pos;
                st.playing = false;
                st.at = now_ms();
            }
            "seek" => {
                st.position = pos;
                st.at = now_ms();
            }
            "load" => {
                let vid = a.video.clone().filter(|v| valid_item_id(&v.id)).ok_or_else(|| bad("Ungültiges Video"))?;
                note = Some(format!("{name} spielt „{}“", vid.title));
                st.video = Some(vid);
                st.position = pos;
                st.playing = true;
                st.at = now_ms();
            }
            // Video zu Ende: nur einmal weiterschalten (alle Mitglieder melden es)
            "ended" | "next" => {
                let current = st.video.as_ref().map(|v| v.id.clone()).unwrap_or_default();
                if a.kind == "ended" && (a.text != current || st.pos_now() < 3.0) {
                    return Ok(Json(json!({ "ok": true })));
                }
                if st.queue.is_empty() {
                    st.playing = false;
                    st.position = 0.0;
                } else {
                    let next = st.queue.remove(0);
                    st.video = Some(next);
                    st.position = 0.0;
                    st.playing = true;
                }
                st.at = now_ms();
            }
            "remove" => {
                if a.index < st.queue.len() {
                    st.queue.remove(a.index);
                }
            }
            "playnow" => {
                if a.index < st.queue.len() {
                    let v = st.queue.remove(a.index);
                    st.video = Some(v);
                    st.position = 0.0;
                    st.playing = true;
                    st.at = now_ms();
                }
            }
            // Gastgeber: Einstellungen der Party
            "settings" if is_host => {
                if let Some(p) = a.public {
                    st.public = p;
                }
                if let Some(g) = a.guests {
                    st.allow_guests = g;
                }
                if let Some(p) = a.perms {
                    st.default_perms = p;
                    // "Für alle übernehmen"
                    if a.flag {
                        let host = st.host.clone();
                        for (k, m) in st.members.iter_mut() {
                            if *k != host {
                                m.perms = p;
                            }
                        }
                    }
                }
            }
            // Gastgeber: Rechte eines Teilnehmers
            "perms" if is_host => {
                if let (Some(p), Some(m)) = (a.perms, st.members.get_mut(&a.text)) {
                    m.perms = p;
                }
            }
            // Gastgeber übergeben
            "host" if is_host => {
                if st.members.contains_key(&a.text) && a.text != st.host {
                    let new_name = st.members.get(&a.text).map(|m| m.name.clone()).unwrap_or_default();
                    st.host = a.text.clone();
                    note = Some(format!("{new_name} ist jetzt Gastgeber"));
                }
            }
            "title" if is_host => {
                let t: String = a.text.trim().chars().take(80).collect();
                if !t.is_empty() {
                    st.title = t;
                }
            }
            "kick" if is_host => {
                if a.text != st.host {
                    if let Some(m) = st.members.remove(&a.text) {
                        note = Some(format!("{} wurde entfernt", m.name));
                    }
                }
            }
            _ => return Err(bad("Unbekannte Aktion")),
        }
        st.by = mid.clone();
    }
    if let Some(n) = note {
        room.system(n).await;
    }
    room.broadcast_state().await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn leave(State(state): State<AppState>, v: Viewer, Path(id): Path<String>) -> Result<Response, ApiError> {
    room_ok(&id)?;
    let Some(room) = state.parties.get(&id).await else { return Ok(Json(json!({ "ok": true })).into_response()) };
    let mid = member_id(&room, &v).await?;
    let (is_host, name) = {
        let st = room.st.lock().await;
        (st.host == mid, st.members.get(&mid).map(|m| m.name.clone()).unwrap_or_default())
    };
    if is_host {
        // Gastgeber beendet die Party für alle
        let _ = room.tx.send(PartyEvent::Closed);
        state.parties.rooms.lock().await.remove(&id);
    } else {
        room.st.lock().await.members.remove(&mid);
        room.system(format!("{name} hat die Party verlassen")).await;
        room.broadcast_state().await;
    }
    let mut resp = Json(json!({ "ok": true })).into_response();
    if mid.starts_with('g') {
        if let Ok(h) = format!("{GUEST_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0").parse() {
            resp.headers_mut().insert(header::SET_COOKIE, h);
        }
    }
    Ok(resp)
}

/// Streams des aktuellen Videos – auch für Gäste
pub async fn streams(State(state): State<AppState>, v: Viewer, Path(id): Path<String>) -> R<StreamInfo> {
    room_ok(&id)?;
    let room = state.parties.get(&id).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Diese Watch-Party gibt es nicht mehr".into()))?;
    member_id(&room, &v).await?;
    let vid = room.st.lock().await.video.as_ref().map(|v| v.id.clone()).ok_or_else(|| bad("Noch kein Video ausgewählt"))?;
    let info = if is_twitch_id(&vid) {
        state.twitch.streams(&vid).await?
    } else {
        (*state.ytdlp.streams(&vid).await?).clone()
    };
    Ok(Json(info))
}

/// Video für die Party aus einer Adresse oder ID (auch für Gäste: YouTube- und Twitch-Links)
pub async fn resolve(State(state): State<AppState>, v: Viewer, Path(id): Path<String>, Query(q): Query<KeyQ>) -> R<VideoItem> {
    room_ok(&id)?;
    let room = state.parties.get(&id).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Diese Watch-Party gibt es nicht mehr".into()))?;
    member_id(&room, &v).await?;
    let input = q.k.trim();
    let lower = input.to_lowercase();
    let get = |k: &str| -> Option<String> {
        let i = input.find(&format!("{k}="))? + k.len() + 1;
        Some(input[i..].split(['&', '#']).next()?.to_string())
    };
    // Twitch
    if let Some(i) = lower.find("twitch.tv/") {
        let rest = &lower[i + 10..];
        let tw_id = if let Some(v) = rest.strip_prefix("videos/") {
            format!("tw:vod:{}", v.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        } else {
            format!("tw:live:{}", rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>())
        };
        let d = state.twitch.details(&tw_id).await?;
        return Ok(Json(VideoItem { id: d.id, title: d.title, author: d.author, author_id: d.author_id, author_thumb: d.author_thumb, thumb: d.thumb, duration: d.duration, live: d.live, ..Default::default() }));
    }
    // YouTube
    let mut vid = get("v").unwrap_or_default();
    for pre in ["youtu.be/", "/shorts/", "/live/", "/embed/"] {
        if let Some(i) = input.find(pre) {
            vid = input[i + pre.len()..].chars().take(11).collect();
        }
    }
    if vid.is_empty() && input.len() == 11 {
        vid = input.to_string();
    }
    if !valid_item_id(&vid) || is_twitch_id(&vid) {
        return Err(bad("Bitte einen YouTube- oder Twitch-Link einfügen"));
    }
    let d = state.yt.video(&vid, None, &crate::yt::Locale::default()).await?;
    Ok(Json(VideoItem {
        id: d.id.clone(),
        title: d.title,
        author: d.author,
        author_id: d.author_id,
        author_thumb: d.author_thumb,
        thumb: crate::proxy::video_thumb(&d.id),
        duration: d.duration,
        live: d.live,
        ..Default::default()
    }))
}

/// Laufende Partys des Kontos
pub async fn mine(State(state): State<AppState>, User(me): User) -> Json<Vec<PartyInfo>> {
    let rooms: Vec<Arc<Room>> = state.parties.rooms.lock().await.values().cloned().collect();
    let mut out = vec![];
    let key = format!("u{}", me.id);
    for r in rooms {
        let st = r.st.lock().await;
        if st.members.contains_key(&key) {
            out.push(info_of(&r, &st, true));
        }
    }
    Json(out)
}

fn info_of(room: &Room, st: &RoomState, joined: bool) -> PartyInfo {
    PartyInfo {
        id: room.id.clone(),
        title: st.title.clone(),
        host: st.members.get(&st.host).map(|m| m.name.clone()).unwrap_or_default(),
        video: st.video.clone(),
        members: st.members.values().filter(|m| m.online > 0).count(),
        joined,
        public: st.public,
        allow_guests: st.allow_guests,
    }
}

/// Öffentliche Partys (für angemeldete Nutzer auffindbar)
pub async fn public_list(State(state): State<AppState>, User(me): User) -> Json<Vec<PartyInfo>> {
    let rooms: Vec<Arc<Room>> = state.parties.rooms.lock().await.values().cloned().collect();
    let key = format!("u{}", me.id);
    let mut out = vec![];
    for r in rooms {
        let st = r.st.lock().await;
        if st.public && st.members.values().any(|m| m.online > 0) {
            out.push(info_of(&r, &st, st.members.contains_key(&key)));
        }
    }
    out.sort_by(|a, b| b.members.cmp(&a.members));
    Json(out)
}
