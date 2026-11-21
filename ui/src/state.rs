//! Globaler Zustand der Oberfläche (Dioxus GlobalSignals).

use crate::api;
use dioxus::prelude::*;
use nrztube_types::*;
use std::collections::HashMap;

pub static AUTH: GlobalSignal<Option<AuthStatus>> = Signal::global(|| None);
pub static SETTINGS: GlobalSignal<Settings> = Signal::global(Settings::default);
pub static SUBS: GlobalSignal<Vec<Subscription>> = Signal::global(Vec::new);
/// Verbundene Konten (Twitch-Login, YouTube-Anmeldung) zum Mitschreiben in Livechats
pub static ACCOUNTS: GlobalSignal<AccountsInfo> = Signal::global(AccountsInfo::default);
/// Gefolgte Twitch-Kanäle
pub static TW_FOLLOWS: GlobalSignal<Vec<TwitchFollow>> = Signal::global(Vec::new);
/// Gefolgte Konten auf TikTok, Instagram und X
pub static SOCIAL_FOLLOWS: GlobalSignal<Vec<SocialFollow>> = Signal::global(Vec::new);
/// Wer gerade auf Twitch live ist (gefolgt oder mit abonniertem YouTube-Kanal verknüpft)
pub static TW_LIVE: GlobalSignal<Vec<VideoItem>> = Signal::global(Vec::new);
/// Livestreams der eigenen YouTube-Abos
pub static YT_LIVE: GlobalSignal<Vec<VideoItem>> = Signal::global(Vec::new);
/// Vorschaukarte neben der Seitenleiste (Livestream, Höhe auf dem Bildschirm)
pub static SIDE_PREVIEW: GlobalSignal<Option<(VideoItem, f64)>> = Signal::global(|| None);
pub static PROFILES: GlobalSignal<Vec<Profile>> = Signal::global(Vec::new);
pub static PLAYLISTS: GlobalSignal<Vec<LocalPlaylist>> = Signal::global(Vec::new);
pub static DEARROW: GlobalSignal<HashMap<String, DeArrow>> = Signal::global(HashMap::new);
pub static TOASTS: GlobalSignal<Vec<Toast>> = Signal::global(Vec::new);
/// Das Video im Player (läuft weiter als Mini-Player, wenn man die Seite verlässt)
pub static NOW: GlobalSignal<Option<NowPlaying>> = Signal::global(|| None);
/// Details zum laufenden Video (Titel, Kanal, ähnliche Videos, Playlist)
pub static DETAILS: GlobalSignal<Option<VideoDetails>> = Signal::global(|| None);
/// Dialog "Zu Playlist hinzufügen"
pub static ADD_TO: GlobalSignal<Option<VideoItem>> = Signal::global(|| None);
pub static SHORTCUTS: GlobalSignal<bool> = Signal::global(|| false);
pub static SIDEBAR: GlobalSignal<bool> = Signal::global(|| false);
/// Twitch-Chat rechts angedockt (breite Bildschirme): Inhalt rückt nach links
pub static CHAT_DOCK: GlobalSignal<bool> = Signal::global(|| false);
/// Livechat eingeklappt (im Player wieder ausklappbar)
pub static CHAT_HIDDEN: GlobalSignal<bool> = Signal::global(|| false);
/// Sprung im Player anfordern (Kapitel, Zeitmarke in Beschreibung/Kommentar)
pub static SEEK: GlobalSignal<Option<f64>> = Signal::global(|| None);
/// Gerade angeklicktes Video: Titel und Kanal sofort zeigen, bis die Details da sind
pub static PENDING: GlobalSignal<Option<VideoItem>> = Signal::global(|| None);
/// Mini-Player eingeklappt (nur die Leiste, Ton läuft weiter)
pub static MINI_COLLAPSED: GlobalSignal<bool> = Signal::global(|| false);
/// Video-IDs in der Stream-Liste (werden in Abo-Feed und Startseite ausgeblendet)
pub static STREAM_IDS: GlobalSignal<std::collections::HashSet<String>> = Signal::global(std::collections::HashSet::new);

#[derive(Clone, Debug, PartialEq, Default, serde::Deserialize)]
pub struct DeArrow {
    pub title: Option<String>,
    pub thumb: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub id: String,
    pub list: Option<String>,
    /// Lokale Playlist (ID) für "Weiter"
    pub local_list: Option<i64>,
    pub start: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u32,
    pub text: String,
    pub error: bool,
}

pub fn toast(text: impl Into<String>) {
    push_toast(text.into(), false);
}

pub fn toast_err(text: impl std::fmt::Display) {
    push_toast(text.to_string(), true);
}

fn push_toast(text: String, error: bool) {
    let id = (js_sys::Math::random() * 1e9) as u32;
    TOASTS.write().push(Toast { id, text, error });
    dioxus::core::spawn_forever(async move {
        gloo_timers::future::TimeoutFuture::new(if error { 6000 } else { 3500 }).await;
        TOASTS.write().retain(|t| t.id != id);
    });
}

pub fn on_unauthorized() {
    if let Some(a) = AUTH.write().as_mut() {
        a.me = None;
    }
}

pub async fn load_auth() {
    if let Ok(a) = api::get::<AuthStatus>("/api/auth/status").await {
        let logged_in = a.me.is_some();
        *AUTH.write() = Some(a);
        if logged_in {
            load_user_data().await;
        }
    } else {
        *AUTH.write() = Some(AuthStatus::default());
    }
}

pub async fn load_user_data() {
    if let Ok(mut s) = api::get::<Settings>("/api/settings").await {
        // Stream-Modus gibt es nur mit eingeschalteten Stream-Funktionen
        if !s.stream_enabled {
            s.stream_mode = false;
        }
        *SETTINGS.write() = s;
    }
    reload_subs().await;
    reload_profiles().await;
    reload_playlists().await;
    reload_tw_follows().await;
    reload_tw_live().await;
    reload_accounts().await;
    reload_social_follows().await;
}

pub async fn reload_social_follows() {
    if let Ok(f) = api::get::<Vec<SocialFollow>>("/api/social/follows").await {
        *SOCIAL_FOLLOWS.write() = f;
    }
}

pub fn is_social_following(platform: &str, handle: &str) -> bool {
    SOCIAL_FOLLOWS.read().iter().any(|f| f.platform == platform && f.handle == handle)
}

/// Konto auf TikTok, Instagram oder X folgen oder entfolgen
pub fn toggle_social_follow(platform: String, handle: String, name: String, avatar: String) {
    let on = !is_social_following(&platform, &handle);
    spawn(async move {
        let r = if on {
            api::post::<_, serde_json::Value>("/api/social/follows", &SocialFollow { platform: platform.clone(), handle: handle.clone(), name: name.clone(), avatar, added: 0 }).await
        } else {
            api::delete::<serde_json::Value>(&format!("/api/social/follows/{platform}/{handle}")).await
        };
        let label = if name.is_empty() { format!("@{handle}") } else { name };
        match r {
            Ok(_) => {
                reload_social_follows().await;
                toast(if on { format!("Du folgst {label}") } else { format!("{label} entfolgt") });
            }
            Err(e) => toast_err(e),
        }
    });
}

pub async fn reload_accounts() {
    if let Ok(a) = api::get::<AccountsInfo>("/api/accounts").await {
        *ACCOUNTS.write() = a;
    }
}

pub async fn reload_tw_follows() {
    if let Ok(f) = api::get::<Vec<TwitchFollow>>("/api/twitch/follows").await {
        *TW_FOLLOWS.write() = f;
    }
}

pub async fn reload_tw_live() {
    // YouTube-Abos, die gerade live sind (unabhängig von Twitch)
    if let Ok(l) = api::get::<Vec<VideoItem>>("/api/youtube/live").await {
        *YT_LIVE.write() = l;
    }
    if !SETTINGS.peek().twitch_enabled {
        return;
    }
    if let Ok(l) = api::get::<Vec<VideoItem>>("/api/twitch/live").await {
        *TW_LIVE.write() = l;
    }
}

pub fn is_tw_following(login: &str) -> bool {
    TW_FOLLOWS.read().iter().any(|f| f.login == login)
}

pub fn is_tw_live(login: &str) -> bool {
    TW_LIVE.read().iter().any(|v| v.author_id == format!("tw:{login}"))
}

/// Twitch-Kanal folgen oder entfolgen
pub fn toggle_tw_follow(login: String, name: String, thumb: String) {
    let on = !is_tw_following(&login);
    spawn(async move {
        let r = if on {
            api::post::<_, serde_json::Value>("/api/twitch/follows", &TwitchFollow { login: login.clone(), name: name.clone(), thumb, added: 0 }).await
        } else {
            api::delete::<serde_json::Value>(&format!("/api/twitch/follows/{login}")).await
        };
        match r {
            Ok(_) => {
                reload_tw_follows().await;
                toast(if on { format!("Du folgst {name} auf Twitch") } else { format!("{name} entfolgt") });
                reload_tw_live().await;
            }
            Err(e) => toast_err(e),
        }
    });
}

pub async fn reload_subs() {
    if let Ok(s) = api::get::<Vec<Subscription>>("/api/subscriptions").await {
        *SUBS.write() = s;
    }
}

pub async fn reload_profiles() {
    if let Ok(p) = api::get::<Vec<Profile>>("/api/profiles").await {
        *PROFILES.write() = p;
    }
}

pub async fn reload_playlists() {
    if let Ok(p) = api::get::<Vec<LocalPlaylist>>("/api/playlists").await {
        *PLAYLISTS.write() = p;
    }
    // Inhalt der Stream-Liste merken
    if let Some(pid) = stream_list_id() {
        if let Ok(p) = api::get::<LocalPlaylist>(&format!("/api/playlists/{pid}")).await {
            *STREAM_IDS.write() = p.videos.into_iter().map(|v| v.id).collect();
        }
    }
}

/// Einstellungen ändern und sofort speichern
pub fn update_settings(f: impl FnOnce(&mut Settings)) {
    let mut s = SETTINGS.read().clone();
    f(&mut s);
    *SETTINGS.write() = s.clone();
    dioxus::core::spawn_forever(async move {
        if let Err(e) = api::put::<_, Settings>("/api/settings", &s).await {
            toast_err(e);
        }
    });
}

/// DeArrow-Titel und -Bilder für eine Liste nachladen
pub fn load_dearrow(ids: Vec<String>) {
    let s = SETTINGS.read();
    if !s.dearrow_titles && !s.dearrow_thumbs {
        return;
    }
    drop(s);
    let missing: Vec<String> = {
        let d = DEARROW.read();
        ids.into_iter().filter(|i| !d.contains_key(i)).collect()
    };
    if missing.is_empty() {
        return;
    }
    dioxus::core::spawn_forever(async move {
        for chunk in missing.chunks(50) {
            let url = format!("/api/dearrow?ids={}", chunk.join(","));
            if let Ok(map) = api::get::<HashMap<String, DeArrow>>(&url).await {
                let mut d = DEARROW.write();
                for id in chunk {
                    d.insert(id.clone(), map.get(id).cloned().unwrap_or_default());
                }
            }
        }
    });
}

pub fn is_subscribed(channel: &str) -> bool {
    SUBS.read().iter().any(|s| s.channel_id == channel)
}

pub async fn toggle_subscription(channel: String, name: String, thumb: String) {
    if is_subscribed(&channel) {
        match api::delete::<serde_json::Value>(&format!("/api/subscriptions/{channel}")).await {
            Ok(_) => toast(format!("{name} nicht mehr abonniert")),
            Err(e) => toast_err(e),
        }
    } else {
        let s = Subscription { channel_id: channel, name: name.clone(), thumb, added: 0 };
        match api::post::<_, serde_json::Value>("/api/subscriptions", &s).await {
            Ok(_) => toast(format!("{name} abonniert")),
            Err(e) => toast_err(e),
        }
    }
    reload_subs().await;
    reload_profiles().await;
}

// ------------------------------------------------------------------ Kontextmenü, Stream-Liste, Bewertungen

/// Kontextmenü eines Videos (Rechtsklick, langes Drücken oder ⋮): Video und Position
pub static CTX: GlobalSignal<Option<(VideoItem, f64, f64)>> = Signal::global(|| None);
/// Schon vorgeladene Videos (Streams)
pub static PREFETCHED: GlobalSignal<std::collections::HashSet<String>> = Signal::global(std::collections::HashSet::new);
/// Bewertungen, die in dieser Sitzung geändert wurden (für Karten und Menü)
pub static RATINGS: GlobalSignal<HashMap<String, i8>> = Signal::global(HashMap::new);

pub const STREAM_LIST: &str = "Stream-Liste";

pub fn special_playlist(title: &str) -> Option<LocalPlaylist> {
    PLAYLISTS.read().iter().find(|p| p.protected && p.title == title).cloned()
}

pub fn stream_list_id() -> Option<i64> {
    special_playlist(STREAM_LIST).map(|p| p.id)
}

/// Videos aus der Stream-Liste ausblenden? (Stream-Funktionen an und nicht auf "Normal")
pub fn hide_queued() -> bool {
    let s = SETTINGS.read();
    s.stream_enabled && s.stream_in_feed != "show"
}

pub fn stream_count() -> u32 {
    special_playlist(STREAM_LIST).map(|p| p.count).unwrap_or(0)
}

/// Video an die Stream-Liste hängen
pub fn add_to_stream(v: VideoItem) {
    dioxus::core::spawn_forever(async move {
        let Some(pid) = stream_list_id() else {
            reload_playlists().await;
            if stream_list_id().is_none() {
                toast_err("Stream-Liste nicht gefunden");
                return;
            }
            return add_to_stream(v);
        };
        match api::post::<_, serde_json::Value>(&format!("/api/playlists/{pid}/videos"), &v).await {
            Ok(r) if r.get("added").and_then(|x| x.as_bool()) == Some(false) => toast("Schon in der Stream-Liste"),
            Ok(_) => toast("Zur Stream-Liste hinzugefügt"),
            Err(e) => toast_err(e),
        }
        reload_playlists().await;
    });
}

pub fn add_to_watch_later(v: VideoItem) {
    let Some(pid) = special_playlist("Später ansehen").map(|p| p.id) else { return };
    dioxus::core::spawn_forever(async move {
        match api::post::<_, serde_json::Value>(&format!("/api/playlists/{pid}/videos"), &v).await {
            Ok(_) => toast("Zu „Später ansehen“ hinzugefügt"),
            Err(e) => toast_err(e),
        }
        reload_playlists().await;
    });
}

/// 1 = Gefällt mir (Favorit), -1 = Gefällt mir nicht, 0 = zurücknehmen
pub fn rate(v: VideoItem, rating: i8) {
    RATINGS.write().insert(v.id.clone(), rating);
    dioxus::core::spawn_forever(async move {
        let body = RateBody { rating, video: v.clone() };
        match api::post::<_, serde_json::Value>(&format!("/api/rating/{}", v.id), &body).await {
            Ok(_) => toast(match rating {
                1 => "Gefällt dir – in den Favoriten",
                -1 => "Wird dir seltener vorgeschlagen",
                _ => "Bewertung entfernt",
            }),
            Err(e) => toast_err(e),
        }
        reload_playlists().await;
    });
}

/// Streams vorladen, damit das Video beim Klick sofort startet
pub fn prefetch(ids: Vec<String>) {
    let ids: Vec<String> = {
        let p = PREFETCHED.read();
        ids.into_iter().filter(|i| !i.is_empty() && !is_twitch_id(i) && !p.contains(i)).collect()
    };
    if ids.is_empty() {
        return;
    }
    PREFETCHED.write().extend(ids.iter().cloned());
    dioxus::core::spawn_forever(async move {
        let _ = api::post::<_, serde_json::Value>("/api/prefetch", &IdsBody { ids }).await;
    });
}
