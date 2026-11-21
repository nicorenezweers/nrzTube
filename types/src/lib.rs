//! Gemeinsame Typen der JSON-API von nrzTube – der Vertrag zwischen Server und Oberfläche.
//!
//! Alle Bild- und Medien-Adressen in diesen Typen zeigen auf den nrzTube-Server selbst
//! (`/img/…`, `/p/…`): der Browser spricht nie direkt mit Google.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ------------------------------------------------------------------ Listen

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoItem {
    pub id: String,
    pub title: String,
    /// Originaltitel, wenn DeArrow den Titel ersetzt hat
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub author_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_thumb: Option<String>,
    #[serde(default)]
    pub thumb: String,
    /// Sekunden
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views: Option<u64>,
    /// Anzeigetext, z. B. "1,2 Mio. Aufrufe"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views_text: Option<String>,
    /// Anzeigetext, z. B. "vor 3 Tagen"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    /// Unix-Sekunden, wenn bekannt
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_ts: Option<i64>,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub upcoming: bool,
    #[serde(default)]
    pub short: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribers: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_count: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaylistItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub author_id: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_count: Option<String>,
    /// Erstes Video (zum direkten Abspielen)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_video: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Item {
    Video(VideoItem),
    Channel(ChannelItem),
    Playlist(PlaylistItem),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
}

// ------------------------------------------------------------------ Video

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    /// Start in Sekunden
    pub start: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoDetails {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub author_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_thumb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribers: Option<String>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub likes_text: Option<String>,
    /// z. B. "12.03.2024"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
    #[serde(default)]
    pub live: bool,
    #[serde(default = "yes")]
    pub family_safe: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    #[serde(default)]
    pub related: Vec<VideoItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_count: Option<String>,
    /// Startpunkt für die Kommentare
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments_token: Option<String>,
    #[serde(default)]
    pub thumb: String,
    /// Wenn das Video zu einer Playlist gehört (Wiedergabe aus Playlist)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist: Option<WatchPlaylist>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WatchPlaylist {
    pub id: String,
    pub title: String,
    pub author: String,
    pub videos: Vec<VideoItem>,
    pub index: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Format {
    pub itag: String,
    /// "1080p60", "128 kbit/s"
    pub label: String,
    pub ext: String,
    /// "av" (Bild+Ton), "video" oder "audio"
    pub kind: String,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub fps: u32,
    #[serde(default)]
    pub bitrate: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Signierte Adresse über den nrzTube-Server
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Caption {
    pub lang: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub auto: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StreamInfo {
    pub id: String,
    /// HLS-Hauptliste (alle Qualitäten) über den nrzTube-Server
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hls: Option<String>,
    #[serde(default)]
    pub formats: Vec<Format>,
    #[serde(default)]
    pub captions: Vec<Caption>,
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default)]
    pub live: bool,
    /// "Meistgesehen"-Kurve (0..1), gleichmässig über die Dauer verteilt
    #[serde(default)]
    pub heatmap: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_limit: Option<u32>,
    /// Schnell geholt (nur Bild und Ton): Untertitel kommen kurz danach mit dem vollen Abruf
    #[serde(default)]
    pub partial: bool,
    /// Vorschaubilder für die Zeitleiste
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storyboard: Option<Storyboard>,
    /// Livestream: läuft seit (Unix-Sekunden)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_since: Option<i64>,
}

/// Bildbögen mit Vorschaubildern (YouTube-Storyboard): `rows`×`cols` Bilder je Bogen,
/// ein Bild alle `interval` Sekunden
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Storyboard {
    pub width: u32,
    pub height: u32,
    pub rows: u32,
    pub cols: u32,
    pub interval: f64,
    pub sheets: Vec<String>,
}

/// Signierte Adressen für externe Player (ohne Anmeldung abrufbar, zeitlich begrenzt)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExternalLinks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hls: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progressive: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    pub title: String,
}

// ------------------------------------------------------------------ Kommentare

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub author: String,
    #[serde(default)]
    pub author_id: String,
    #[serde(default)]
    pub author_thumb: String,
    pub text: String,
    #[serde(default)]
    pub likes: String,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub reply_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replies_token: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub hearted: bool,
    #[serde(default)]
    pub by_owner: bool,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CommentPage {
    pub comments: Vec<Comment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<String>,
    /// Umschalten auf "Neueste zuerst"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newest_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_token: Option<String>,
}

// ------------------------------------------------------------------ Kanal

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(default)]
    pub avatar: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribers: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_count: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub family_safe: bool,
    /// Vorhandene Reiter: videos, shorts, streams, playlists, posts
    #[serde(default)]
    pub tabs: Vec<String>,
    #[serde(default)]
    pub links: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub author: String,
    #[serde(default)]
    pub author_id: String,
    #[serde(default)]
    pub author_thumb: String,
    pub text: String,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub likes: String,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub images: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoItem>,
    #[serde(default)]
    pub poll: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_votes: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaylistDetails {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub author_id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_count: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views: Option<String>,
    pub videos: Vec<VideoItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
}

// ------------------------------------------------------------------ SponsorBlock

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub category: String,
    /// "skip", "mute", "full", "poi", "chapter"
    pub action: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub uuid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ------------------------------------------------------------------ Konto

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Me {
    pub id: i64,
    pub username: String,
    pub admin: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuthStatus {
    /// Noch kein Konto: das erste wird Admin
    pub setup: bool,
    pub registration_open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub me: Option<Me>,
    pub version: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    /// Einladungscode (Registrierung auch bei geschlossener Anmeldung)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub admin: bool,
    pub created: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminInfo {
    pub users: Vec<UserInfo>,
    pub registration_open: bool,
    pub ytdlp_version: String,
    /// YouTube-Anmeldung (cookies.txt) für altersbeschränkte Videos hinterlegt
    #[serde(default)]
    pub youtube_cookies: bool,
    /// Social-Plattformen mit hinterlegter Anmeldung ("tiktok", "instagram", "x")
    #[serde(default)]
    pub social_cookies: Vec<String>,
}

// ------------------------------------------------------------------ Bibliothek

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
    pub channel_id: String,
    pub name: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default)]
    pub added: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub channels: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalPlaylist {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub protected: bool,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub thumb: String,
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub videos: Vec<VideoItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub video: VideoItem,
    pub watched: i64,
    /// Zuletzt angesehene Stelle in Sekunden
    #[serde(default)]
    pub progress: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryUpdate {
    pub video: VideoItem,
    #[serde(default)]
    pub progress: f64,
    /// Seit der letzten Meldung tatsächlich abgespielte Sekunden (für die Empfehlungen)
    #[serde(default)]
    pub played: f64,
    /// Länge des Videos in Sekunden
    #[serde(default)]
    pub duration: f64,
    /// Video wurde gerade geöffnet und gestartet
    #[serde(default)]
    pub opened: bool,
}

/// Bewertung eines Videos: 1 = Gefällt mir (gleich Favorit), -1 = Gefällt mir nicht, 0 = keine
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rating {
    pub rating: i8,
    /// Dislikes von Return YouTube Dislike (über den Server abgefragt)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dislikes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub likes: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RateBody {
    pub rating: i8,
    pub video: VideoItem,
}

/// Vorschlag auf der Startseite mit Begründung
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecoItem {
    pub video: VideoItem,
    #[serde(default)]
    pub reason: String,
    /// "subs" (abonnierter Kanal), "known" (oft geschaut), "new" (neu entdecken)
    #[serde(default)]
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HomeFeed {
    /// Angefangene Videos
    #[serde(default)]
    pub resume: Vec<HistoryEntry>,
    #[serde(default)]
    pub items: Vec<RecoItem>,
    /// Noch keine Daten: Vorschläge aus Trends und Abos
    #[serde(default)]
    pub cold: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TasteChannel {
    pub channel_id: String,
    pub name: String,
    #[serde(default)]
    pub thumb: String,
    pub score: f32,
    pub minutes: f32,
    pub videos: u32,
    #[serde(default)]
    pub likes: u32,
    #[serde(default)]
    pub dislikes: u32,
}

/// Was nrzTube über deinen Geschmack weiss (alles nur in der lokalen Datenbank)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Taste {
    pub channels: Vec<TasteChannel>,
    pub minutes: f32,
    pub videos: u32,
    pub likes: u32,
    pub dislikes: u32,
    #[serde(default)]
    pub searches: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IdsBody {
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportResult {
    pub added: u32,
    pub skipped: u32,
    #[serde(default)]
    pub format: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
}

// ------------------------------------------------------------------ Einstellungen

/// Alle Einstellungen eines Kontos. Unbekannte/fehlende Felder bekommen den Standardwert,
/// damit alte gespeicherte Einstellungen nach Updates weiter funktionieren.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Allgemein
    pub region: String,
    pub language: String,
    /// "home", "feed", "trending", "subscriptions", "playlists", "history"
    pub start_page: String,
    pub open_in_new_window: bool,
    pub list_view: bool,

    // Player
    pub autoplay: bool,
    pub autoplay_next: bool,
    /// "auto", "2160", "1440", "1080", "720", "480", "360", "240", "144", "audio"
    pub quality: String,
    /// "avc1" (H.264, sparsam) oder "any" (auch VP9 bis 4K)
    pub codec: String,
    pub speed: f64,
    pub volume: f64,
    pub remember_position: bool,
    pub captions: bool,
    pub caption_lang: String,
    /// Bevorzugte Audiospur bei mehrsprachigen Videos ("de", "en", … oder "original")
    pub audio_lang: String,
    pub loop_video: bool,
    pub theater: bool,
    /// "vlc", "mpv", "m3u", "copy"
    pub external_player: String,
    pub skip_seconds: u32,
    pub screenshot_format: String,
    pub mini_player: bool,

    // Datenschutz
    pub save_history: bool,
    pub save_position: bool,
    pub family_friendly: bool,
    pub hidden_channels: Vec<String>,
    pub hidden_keywords: Vec<String>,
    /// Startseite mit Vorschlägen aus dem eigenen Verlauf
    pub reco_enabled: bool,
    /// Dislike-Zahlen von Return YouTube Dislike zeigen
    pub ryd: bool,

    // Twitch
    pub twitch_enabled: bool,
    /// Chat neben Livestreams
    pub twitch_chat: bool,
    /// "Jetzt live"-Leiste auf Startseite und im Feed
    pub twitch_live_row: bool,
    /// Ausgeblendete Twitch-Kategorien (wie unwanted-twitch)
    pub twitch_hidden_games: Vec<String>,
    /// Reaktionsrichtlinien von Can I React unter Videos anzeigen
    pub can_i_react: bool,

    // Social (TikTok, Instagram, X)
    pub social_enabled: bool,
    pub social_tiktok: bool,
    pub social_instagram: bool,
    pub social_x: bool,

    // Stream-Modus
    /// Stream-Liste und Stream-Modus überhaupt anbieten (Standard: aus)
    pub stream_enabled: bool,
    pub stream_mode: bool,
    /// Im Stream-Modus nichts in Verlauf und Empfehlungsdaten speichern
    pub stream_private: bool,
    /// Fertig geschaute Videos aus der Stream-Liste entfernen
    pub stream_remove_done: bool,
    /// Videos, die schon in der Stream-Liste sind, in Abo-Feed und Startseite: "hide" oder "show"
    pub stream_in_feed: String,

    // SponsorBlock
    pub sb_enabled: bool,
    /// Kategorie -> "skip", "manual", "show", "off"
    pub sb_categories: BTreeMap<String, String>,
    pub sb_show_bar: bool,
    pub sb_notify: bool,
    pub sb_min_length: f64,

    // DeArrow
    pub dearrow_titles: bool,
    pub dearrow_thumbs: bool,
    pub dearrow_show_original: bool,

    // Ablenkungsfrei
    pub hide_trending: bool,
    pub hide_comments: bool,
    pub hide_related: bool,
    pub hide_description: bool,
    pub hide_views: bool,
    pub hide_likes: bool,
    pub hide_subscriber_counts: bool,
    pub hide_channel_avatars: bool,
    pub hide_shorts: bool,
    pub hide_live: bool,
    pub hide_upcoming: bool,
    pub hide_watched: bool,
    pub hide_chapters: bool,
    pub hide_posts: bool,
    pub hide_playlists: bool,
    pub hide_channel_playlists: bool,
    pub hide_comment_avatars: bool,
    pub hide_upload_date: bool,
    pub hide_search_suggestions: bool,
    pub hide_end_suggestions: bool,
    pub hide_heatmap: bool,
    pub blur_thumbnails: bool,
    pub hide_sidebar_subscriptions: bool,
}

pub const SB_CATEGORIES: &[(&str, &str, &str)] = &[
    ("sponsor", "Sponsor", "#00d400"),
    ("selfpromo", "Eigenwerbung", "#ffff00"),
    ("interaction", "Interaktion (Abo-Aufruf)", "#cc00ff"),
    ("intro", "Intro / Pause", "#00ffff"),
    ("outro", "Abspann", "#0202ed"),
    ("preview", "Vorschau / Rückblick", "#008fd6"),
    ("hook", "Hook / Begrüssung", "#395699"),
    ("filler", "Füllmaterial", "#7300FF"),
    ("music_offtopic", "Nicht-Musik-Teil", "#ff9900"),
    ("poi_highlight", "Highlight", "#ff1684"),
];

impl Default for Settings {
    fn default() -> Self {
        let mut sb = BTreeMap::new();
        for (cat, _, _) in SB_CATEGORIES {
            let v = match *cat {
                "sponsor" | "selfpromo" | "interaction" => "skip",
                "poi_highlight" => "show",
                "music_offtopic" => "skip",
                _ => "manual",
            };
            sb.insert((*cat).to_string(), v.to_string());
        }
        Self {
            region: "CH".into(),
            language: "de".into(),
            start_page: "home".into(),
            open_in_new_window: false,
            list_view: false,
            autoplay: true,
            autoplay_next: false,
            quality: "auto".into(),
            codec: "avc1".into(),
            speed: 1.0,
            volume: 1.0,
            remember_position: true,
            captions: false,
            caption_lang: "de".into(),
            audio_lang: "de".into(),
            loop_video: false,
            theater: true,
            external_player: "vlc".into(),
            skip_seconds: 10,
            screenshot_format: "png".into(),
            mini_player: true,
            save_history: true,
            save_position: true,
            family_friendly: false,
            hidden_channels: Vec::new(),
            hidden_keywords: Vec::new(),
            reco_enabled: true,
            ryd: true,
            twitch_enabled: true,
            twitch_chat: true,
            twitch_live_row: true,
            twitch_hidden_games: Vec::new(),
            can_i_react: true,
            social_enabled: true,
            social_tiktok: true,
            social_instagram: true,
            social_x: true,
            stream_enabled: false,
            stream_mode: false,
            stream_private: true,
            stream_remove_done: true,
            stream_in_feed: "hide".into(),
            sb_enabled: true,
            sb_categories: sb,
            sb_show_bar: true,
            sb_notify: true,
            sb_min_length: 0.0,
            dearrow_titles: true,
            dearrow_thumbs: true,
            dearrow_show_original: true,
            hide_trending: false,
            hide_comments: false,
            hide_related: false,
            hide_description: false,
            hide_views: false,
            hide_likes: false,
            hide_subscriber_counts: false,
            hide_channel_avatars: false,
            hide_shorts: false,
            hide_live: false,
            hide_upcoming: false,
            hide_watched: false,
            hide_chapters: false,
            hide_posts: false,
            hide_playlists: false,
            hide_channel_playlists: false,
            hide_comment_avatars: false,
            hide_upload_date: false,
            hide_search_suggestions: false,
            hide_end_suggestions: false,
            hide_heatmap: false,
            blur_thumbnails: false,
            hide_sidebar_subscriptions: false,
        }
    }
}

/// Kategorien der Trends-Seite (YouTube hat die klassische Trends-Seite 2025 abgeschafft;
/// nrzTube zeigt stattdessen die offiziellen Themen-Kanäle von YouTube).
pub const TRENDING_CATEGORIES: &[(&str, &str)] = &[
    ("now", "Angesagt"),
    ("music", "Musik"),
    ("gaming", "Gaming"),
    ("live", "Live"),
    ("sports", "Sport"),
    ("news", "Nachrichten"),
    ("learning", "Lernen"),
    ("fashion", "Mode & Beauty"),
];

// ------------------------------------------------------------------ Twitch
//
// Twitch-Inhalte laufen durch dieselben Listen und denselben Player wie YouTube. Ihre IDs
// tragen ein Präfix: `tw:live:<login>`, `tw:vod:<id>`, `tw:clip:<slug>`; Kanäle `tw:<login>`.

pub fn is_twitch_id(id: &str) -> bool {
    id.starts_with("tw:")
}

/// Nicht YouTube (Twitch)
pub fn is_external_id(id: &str) -> bool {
    is_twitch_id(id)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TwitchStream {
    pub title: String,
    #[serde(default)]
    pub viewers: u64,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub thumb: String,
    /// Unix-Sekunden
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TwitchChannel {
    pub login: String,
    pub name: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followers: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<TwitchStream>,
    /// (Titel, Adresse)
    #[serde(default)]
    pub socials: Vec<(String, String)>,
    /// Verknüpfte YouTube-Kanäle
    #[serde(default)]
    pub links: Vec<ChannelLink>,
}

/// Verknüpfung YouTube-Kanal <-> Twitch-Kanal
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelLink {
    pub yt_id: String,
    #[serde(default)]
    pub yt_name: String,
    #[serde(default)]
    pub yt_thumb: String,
    pub tw_login: String,
    #[serde(default)]
    pub tw_name: String,
    #[serde(default)]
    pub tw_thumb: String,
    /// "manual", "youtube" (Links/Beschreibung des Kanals), "twitch" (Social-Links bei Twitch),
    /// "video" (Videobeschreibung), "name" (gleicher Name)
    #[serde(default)]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<TwitchStream>,
}

impl ChannelLink {
    pub fn source_label(&self) -> &'static str {
        match self.source.as_str() {
            "manual" => "von dir verknüpft",
            "youtube" => "im YouTube-Kanal verlinkt",
            "twitch" => "im Twitch-Kanal verlinkt",
            "video" => "in Videobeschreibungen verlinkt",
            "name" => "gleicher Name",
            _ => "verknüpft",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TwitchFollow {
    pub login: String,
    pub name: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default)]
    pub added: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkBody {
    pub yt_id: String,
    pub tw_login: String,
}

/// Eine Chat-Zeile (Server-Sent Events von /api/twitch/chat/<login>)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatLine {
    pub id: String,
    pub user: String,
    #[serde(default)]
    pub color: String,
    /// Text und Emotes im Wechsel
    pub parts: Vec<ChatPart>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum ChatPart {
    Text { t: String },
    Emote { name: String, url: String },
}

// ------------------------------------------------------------------ Verbundene Konten, Chat schreiben

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountsInfo {
    /// Angemeldeter Twitch-Login
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twitch: Option<String>,
    /// YouTube-Anmeldung (cookies.txt) für den Livechat hinterlegt
    #[serde(default)]
    pub youtube: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub interval: u32,
    #[serde(default)]
    pub expires_in: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatSend {
    pub text: String,
}

// ------------------------------------------------------------------ Watch-Party (gemeinsam schauen)

/// Rechte in einer Watch-Party (der Gastgeber hat immer alle)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartyPerms {
    /// Abspielen, Pause, Springen, nächstes Video
    #[serde(default)]
    pub control: bool,
    /// Videos einreihen
    #[serde(default)]
    pub queue: bool,
    /// Warteschlange verwalten (entfernen, vorziehen)
    #[serde(default)]
    pub manage: bool,
    #[serde(default)]
    pub chat: bool,
}

impl Default for PartyPerms {
    fn default() -> Self {
        Self { control: true, queue: true, manage: false, chat: true }
    }
}

impl PartyPerms {
    pub const ALL: PartyPerms = PartyPerms { control: true, queue: true, manage: true, chat: true };
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyMember {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub guest: bool,
    #[serde(default)]
    pub host: bool,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub perms: PartyPerms,
}

/// Zustand eines Raums. Position gilt zum Zeitpunkt `at` (Server-Millisekunden).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyState {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoItem>,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub at: i64,
    /// Serverzeit beim Senden (zum Ausgleichen der Uhren)
    #[serde(default)]
    pub now: i64,
    #[serde(default)]
    pub queue: Vec<VideoItem>,
    #[serde(default)]
    pub members: Vec<PartyMember>,
    /// Für alle sichtbar (Liste öffentlicher Partys)
    #[serde(default)]
    pub public: bool,
    /// Gäste ohne Konto dürfen beitreten
    #[serde(default)]
    pub allow_guests: bool,
    /// Rechte neuer Teilnehmer
    #[serde(default)]
    pub default_perms: PartyPerms,
    /// Wer zuletzt etwas geändert hat (Mitglieds-ID)
    #[serde(default)]
    pub by: String,
    /// Nur für Mitglieder: Einladungslink-Schlüssel und eigene ID
    #[serde(default)]
    pub invite: String,
    #[serde(default)]
    pub me: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyChatMsg {
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub system: bool,
}

/// Ereignis über SSE
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum PartyEvent {
    State { s: PartyState },
    Chat { m: PartyChatMsg },
    Closed,
}

/// Aktion eines Mitglieds: play, pause, seek, load, queue, next, remove, ended, controls, chat, rate
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyAction {
    pub kind: String,
    #[serde(default)]
    pub position: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoItem>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub flag: bool,
    /// Einstellungen des Gastgebers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perms: Option<PartyPerms>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guests: Option<bool>,
}

/// Für die Beitrittsseite (ohne Mitgliedschaft abrufbar, mit Einladungsschlüssel)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyInfo {
    pub id: String,
    pub title: String,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoItem>,
    #[serde(default)]
    pub members: usize,
    /// Schon Mitglied (angemeldet oder Gast-Cookie)?
    #[serde(default)]
    pub joined: bool,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub allow_guests: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyJoin {
    pub k: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartyCreate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoItem>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub public: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    pub code: String,
    pub created: i64,
    pub expires: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_by: Option<String>,
}

// ------------------------------------------------------------------ Recap (Sehzeit-Statistik)

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecapChannel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub thumb: String,
    pub seconds: f64,
    #[serde(default)]
    pub videos: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecapVideo {
    pub video: VideoItem,
    pub seconds: f64,
    #[serde(default)]
    pub opens: i64,
    /// Zuletzt gesehen (Unix-Sekunden)
    #[serde(default)]
    pub last: i64,
    /// Stelle und Länge in Sekunden
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub duration: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecapDay {
    /// JJJJ-MM-TT
    pub day: String,
    pub seconds: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recap {
    /// "7", "30", "365" oder "all"
    pub range: String,
    pub total: f64,
    pub videos: u32,
    pub channels: u32,
    #[serde(default)]
    pub active_days: u32,
    /// Sehzeit pro Tag im Zeitraum (lückenlos)
    #[serde(default)]
    pub days: Vec<RecapDay>,
    /// Sehzeit nach Uhrzeit (24 Werte)
    #[serde(default)]
    pub hours: Vec<f64>,
    #[serde(default)]
    pub channels_list: Vec<RecapChannel>,
    #[serde(default)]
    pub videos_list: Vec<RecapVideo>,
    /// Ab wann es Tagesdaten gibt (JJJJ-MM-TT)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// Details zu einem Kanal oder Video
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecapDetail {
    pub title: String,
    #[serde(default)]
    pub thumb: String,
    pub total: f64,
    #[serde(default)]
    pub days: Vec<RecapDay>,
    #[serde(default)]
    pub videos: Vec<RecapVideo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<RecapVideo>,
}

// ------------------------------------------------------------------ Can I React (Reaktionsrichtlinien)

/// Regeln für Reaktionen im Stream
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReactStreamRules {
    #[serde(default)]
    pub allowed: Option<bool>,
    #[serde(default)]
    pub generally_allowed: Option<bool>,
    /// Erst so viele Stunden nach dem Upload erlaubt
    #[serde(default)]
    pub after_hours: Option<f64>,
    #[serde(default)]
    pub sponsor_skips_allowed: Option<bool>,
    #[serde(default)]
    pub credit_chat: bool,
    #[serde(default)]
    pub custom: Vec<String>,
}

/// Regeln für Reaction-Videos
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReactVideoRules {
    #[serde(default)]
    pub allowed: Option<bool>,
    #[serde(default)]
    pub generally_allowed: Option<bool>,
    #[serde(default)]
    pub after_hours: Option<f64>,
    #[serde(default)]
    pub sponsor_cut_allowed: Option<bool>,
    #[serde(default)]
    pub credit_description: bool,
    #[serde(default)]
    pub title_must_include: bool,
    #[serde(default)]
    pub monetization_allowed: Option<bool>,
    #[serde(default)]
    pub splitting_allowed: Option<bool>,
    #[serde(default)]
    pub custom: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReactSegment {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReactReaction {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default)]
    pub views: u64,
    /// "youtube" / "twitch"
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReactInfo {
    /// Der Kanal hat Richtlinien hinterlegt
    #[serde(default)]
    pub has_rules: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_text: Option<String>,
    /// Upload-Zeitpunkt (Unix-Sekunden) für "erst nach X Stunden"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uploaded: Option<i64>,
    #[serde(default)]
    pub stream: ReactStreamRules,
    #[serde(default)]
    pub video: ReactVideoRules,
    /// Placements, die nicht übersprungen werden dürfen
    #[serde(default)]
    pub sponsor: Vec<ReactSegment>,
    /// Abschnitte, die gegen die Plattform-Regeln (TOS) verstossen könnten
    #[serde(default)]
    pub tos: Vec<ReactSegment>,
    /// Wenn dieses Video selbst eine Reaktion ist: das Original
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<VideoItem>,
    #[serde(default)]
    pub reactions: Vec<ReactReaction>,
}

// ------------------------------------------------------------------ Social (TikTok, Instagram, X)

/// Plattformen im Social-Bereich: "tiktok", "instagram", "x"
pub const SOCIAL_PLATFORMS: &[&str] = &["tiktok", "instagram", "x"];

/// Bild oder Video eines Beitrags (Adressen laufen über den nrzTube-Server)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SocialMedia {
    /// "image" oder "video"
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SocialPost {
    pub platform: String,
    pub id: String,
    /// Handle ohne @
    pub author: String,
    #[serde(default)]
    pub author_name: String,
    #[serde(default)]
    pub author_avatar: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub published: i64,
    #[serde(default)]
    pub media: Vec<SocialMedia>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub likes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reposts: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// Geteilt von (X: Retweet)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repost_by: Option<String>,
    /// Adresse auf der Plattform selbst
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SocialProfile {
    pub platform: String,
    pub handle: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followers: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub following: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_count: Option<u64>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub posts: Vec<SocialPost>,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SocialFollow {
    pub platform: String,
    pub handle: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub added: i64,
}

/// Gemischter Feed der gefolgten Konten; Fehler einzelner Konten stehen daneben
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SocialFeed {
    pub posts: Vec<SocialPost>,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Pro Plattform: ist eine Anmeldung (cookies.txt) hinterlegt, braucht sie eine?
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SocialStatus {
    pub platform: String,
    pub login: bool,
    /// Ohne Anmeldung gehen Profile nicht (Instagram) oder nur eingeschränkt (X)
    pub needs_login: bool,
}

impl Settings {
    /// Plattform im Social-Bereich eingeschaltet?
    pub fn social_on(&self, platform: &str) -> bool {
        self.social_enabled
            && match platform {
                "tiktok" => self.social_tiktok,
                "instagram" => self.social_instagram,
                "x" => self.social_x,
                _ => false,
            }
    }
}
