//! Twitch über die öffentliche GraphQL-Schnittstelle der Twitch-Webseite (ohne Konto):
//! Kanäle, Livestreams, VODs, Clips, Suche und Chat (anonym über IRC). Bilder und Videos laufen
//! wie bei YouTube über den nrzTube-Server; Werbeblöcke in Livestreams werden herausgefiltert.

use crate::proxy::{img, media};
use anyhow::{anyhow, Result};
use nrztube_types::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Client-ID der Twitch-Webseite (öffentlich, steht in deren JavaScript)
const CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";
const GQL: &str = "https://gql.twitch.tv/gql";

pub struct Twitch {
    http: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
    /// Offene Schreib-Verbindungen pro Konto (Token, Kanal zur Aufgabe)
    writers: Mutex<HashMap<i64, (String, tokio::sync::mpsc::Sender<WriteReq>)>>,
    /// Ersatz-Player-Typ gegen Werbung pro Kanal (seit, Typ, Varianten)
    #[allow(clippy::type_complexity)]
    backups: Mutex<HashMap<String, (Instant, String, Vec<(String, String)>)>>,
}

pub fn valid_login(s: &str) -> bool {
    !s.is_empty() && s.len() <= 25 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn s(v: &Value, ptr: &str) -> String {
    v.pointer(ptr).and_then(Value::as_str).unwrap_or("").to_string()
}

fn n(v: &Value, ptr: &str) -> Option<u64> {
    v.pointer(ptr).and_then(Value::as_u64)
}

fn ts(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|d| d.timestamp())
}

fn thousands(x: u64) -> String {
    let s = x.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

fn viewers_text(x: u64) -> String {
    format!("{} Zuschauer", thousands(x))
}

const STREAM_FIELDS: &str = "stream{ id title viewersCount createdAt previewImageURL(width:640,height:360) game{ displayName } }";

impl Twitch {
    pub fn new(http: reqwest::Client) -> Arc<Self> {
        Arc::new(Self { http, cache: Mutex::new(HashMap::new()), writers: Mutex::new(HashMap::new()), backups: Mutex::new(HashMap::new()) })
    }

    async fn gql(&self, query: &str, vars: Value) -> Result<Value> {
        let res = self
            .http
            .post(GQL)
            .header("Client-ID", CLIENT_ID)
            .json(&json!({ "query": query, "variables": vars }))
            .timeout(Duration::from_secs(12))
            .send()
            .await?;
        let v: Value = res.json().await?;
        if let Some(e) = v.pointer("/errors/0/message").and_then(Value::as_str) {
            // Fehler zählen, wenn keine Teilantwort mit Inhalt da ist
            let empty = v.get("data").and_then(Value::as_object).map(|m| m.values().all(Value::is_null)).unwrap_or(true);
            if empty {
                return Err(anyhow!("Twitch: {e}"));
            }
        }
        Ok(v.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Mit kurzem Zwischenspeicher (Livestatus ändert sich, Kanaldaten kaum)
    async fn cached(&self, key: String, ttl: Duration, query: &str, vars: Value) -> Result<Value> {
        if let Some((t, v)) = self.cache.lock().await.get(&key) {
            if t.elapsed() < ttl {
                return Ok(v.clone());
            }
        }
        let v = self.gql(query, vars).await?;
        let mut c = self.cache.lock().await;
        if c.len() > 2000 {
            c.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(600));
        }
        c.insert(key, (Instant::now(), v.clone()));
        Ok(v)
    }

    fn stream_of(u: &Value) -> Option<TwitchStream> {
        let st = u.get("stream").filter(|x| !x.is_null())?;
        Some(TwitchStream {
            title: s(st, "/title"),
            viewers: n(st, "/viewersCount").unwrap_or(0),
            game: s(st, "/game/displayName"),
            thumb: img(&s(st, "/previewImageURL")),
            started: ts(&s(st, "/createdAt")),
        })
    }

    fn channel_of(u: &Value) -> TwitchChannel {
        let banner = s(u, "/bannerImageURL");
        TwitchChannel {
            login: s(u, "/login"),
            name: s(u, "/displayName"),
            avatar: img(&s(u, "/profileImageURL")),
            banner: (!banner.is_empty()).then(|| img(&banner)),
            description: s(u, "/description"),
            followers: n(u, "/followers/totalCount"),
            live: Self::stream_of(u),
            socials: u
                .pointer("/channel/socialMedias")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|m| (s(m, "/title").trim().to_string(), s(m, "/url"))).filter(|(_, u)| !u.is_empty()).collect())
                .unwrap_or_default(),
            links: vec![],
        }
    }

    pub async fn channel(&self, login: &str) -> Result<TwitchChannel> {
        let login = login.to_lowercase();
        let q = format!(
            "query($l:String!){{ user(login:$l){{ id login displayName description profileImageURL(width:300) bannerImageURL followers{{totalCount}} {STREAM_FIELDS} channel{{ socialMedias{{ title url }} }} }} }}"
        );
        let v = self.cached(format!("ch:{login}"), Duration::from_secs(60), &q, json!({ "l": login })).await?;
        let u = v.get("user").filter(|u| !u.is_null()).ok_or_else(|| anyhow!("Twitch-Kanal nicht gefunden"))?;
        Ok(Self::channel_of(u))
    }

    /// Livestatus vieler Kanäle auf einmal (je 100)
    pub async fn live_many(&self, logins: &[String]) -> Vec<TwitchChannel> {
        let mut out = vec![];
        let q = format!("query($l:[String!]){{ users(logins:$l){{ login displayName profileImageURL(width:70) {STREAM_FIELDS} }} }}");
        for chunk in logins.chunks(100) {
            let mut key: Vec<String> = chunk.to_vec();
            key.sort();
            if let Ok(v) = self.cached(format!("live:{}", key.join(",")), Duration::from_secs(60), &q, json!({ "l": chunk })).await {
                for u in v.get("users").and_then(Value::as_array).into_iter().flatten().filter(|u| !u.is_null()) {
                    out.push(Self::channel_of(u));
                }
            }
        }
        out
    }

    pub async fn search(&self, q: &str) -> Result<Vec<TwitchChannel>> {
        let query = format!("query($q:String!){{ searchUsers(userQuery:$q, first:12){{ edges{{ node{{ login displayName description profileImageURL(width:150) followers{{totalCount}} {STREAM_FIELDS} }} }} }} }}");
        let v = self.cached(format!("search:{}", q.to_lowercase()), Duration::from_secs(300), &query, json!({ "q": q })).await?;
        let mut list: Vec<TwitchChannel> = v
            .pointer("/searchUsers/edges")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("node").filter(|x| !x.is_null()))
            .map(Self::channel_of)
            .collect();
        // Live und grosse Kanäle zuerst, exakte Treffer ganz vorne
        let ql = q.to_lowercase().replace(' ', "");
        list.sort_by_key(|c| (c.login != ql, std::cmp::Reverse(c.live.is_some() as u64 * 1_000_000_000 + c.followers.unwrap_or(0))));
        Ok(list)
    }

    /// Meistgesehene Livestreams (optional einer Kategorie)
    pub async fn top(&self, game: Option<&str>) -> Result<Vec<VideoItem>> {
        let v = match game.filter(|g| !g.is_empty()) {
            Some(g) => {
                let q = "query($g:String!){ game(name:$g){ streams(first:30){ edges{ node{ id title viewersCount createdAt previewImageURL(width:640,height:360) game{displayName} broadcaster{ login displayName profileImageURL(width:70) } } } } } }";
                let v = self.cached(format!("top:{g}"), Duration::from_secs(120), q, json!({ "g": g })).await?;
                v.pointer("/game/streams/edges").cloned()
            }
            None => {
                let q = "query{ streams(first:30){ edges{ node{ id title viewersCount createdAt previewImageURL(width:640,height:360) game{displayName} broadcaster{ login displayName profileImageURL(width:70) } } } } }";
                let v = self.cached("top:".into(), Duration::from_secs(120), q, json!({})).await?;
                v.pointer("/streams/edges").cloned()
            }
        };
        Ok(v.and_then(|e| e.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|e| e.get("node"))
            .map(|st| {
                let b = st.get("broadcaster").cloned().unwrap_or(Value::Null);
                live_item(
                    &s(&b, "/login"),
                    &s(&b, "/displayName"),
                    &img(&s(&b, "/profileImageURL")),
                    &TwitchStream {
                        title: s(st, "/title"),
                        viewers: n(st, "/viewersCount").unwrap_or(0),
                        game: s(st, "/game/displayName"),
                        thumb: img(&s(st, "/previewImageURL")),
                        started: ts(&s(st, "/createdAt")),
                    },
                )
            })
            .collect())
    }

    /// Beliebte Kategorien
    pub async fn games(&self) -> Result<Vec<(String, String, u64)>> {
        let q = "query{ games(first:24){ edges{ node{ displayName boxArtURL(width:144,height:192) viewersCount } } } }";
        let v = self.cached("games".into(), Duration::from_secs(600), q, json!({})).await?;
        Ok(v.pointer("/games/edges")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("node"))
            .map(|g| (s(g, "/displayName"), img(&s(g, "/boxArtURL")), n(g, "/viewersCount").unwrap_or(0)))
            .collect())
    }

    /// VODs ("archive", "highlight", "upload") oder Clips ("clips")
    pub async fn videos(&self, login: &str, kind: &str) -> Result<Vec<VideoItem>> {
        let login = login.to_lowercase();
        let ch = self.channel(&login).await.ok();
        let (name, avatar) = ch.map(|c| (c.name, c.avatar)).unwrap_or_else(|| (login.clone(), String::new()));
        if kind == "clips" {
            let q = "query($l:String!){ user(login:$l){ clips(first:30, criteria:{period:LAST_MONTH, sort:VIEWS_DESC}){ edges{ node{ slug title durationSeconds viewCount createdAt thumbnailURL } } } } }";
            let v = self.cached(format!("clips:{login}"), Duration::from_secs(600), q, json!({ "l": login })).await?;
            return Ok(v
                .pointer("/user/clips/edges")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|e| e.get("node"))
                .map(|c| VideoItem {
                    id: format!("tw:clip:{}", s(c, "/slug")),
                    title: s(c, "/title"),
                    author: name.clone(),
                    author_id: format!("tw:{login}"),
                    author_thumb: Some(avatar.clone()).filter(|a| !a.is_empty()),
                    thumb: img(&s(c, "/thumbnailURL")),
                    duration: c.pointer("/durationSeconds").and_then(Value::as_f64).map(|d| d.round() as u32),
                    views: n(c, "/viewCount"),
                    published_ts: ts(&s(c, "/createdAt")),
                    ..Default::default()
                })
                .collect());
        }
        let ty = match kind {
            "highlight" => "HIGHLIGHT",
            "upload" => "UPLOAD",
            _ => "ARCHIVE",
        };
        let q = "query($l:String!, $t:BroadcastType){ user(login:$l){ videos(first:30, type:$t, sort:TIME){ edges{ node{ id title lengthSeconds viewCount publishedAt previewThumbnailURL(width:640,height:360) game{displayName} } } } } }";
        let v = self.cached(format!("vods:{login}:{ty}"), Duration::from_secs(300), q, json!({ "l": login, "t": ty })).await?;
        Ok(v.pointer("/user/videos/edges")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("node"))
            .map(|x| VideoItem {
                id: format!("tw:vod:{}", s(x, "/id")),
                title: s(x, "/title"),
                author: name.clone(),
                author_id: format!("tw:{login}"),
                author_thumb: Some(avatar.clone()).filter(|a| !a.is_empty()),
                thumb: {
                    let t = s(x, "/previewThumbnailURL");
                    if t.contains("404_processing") || t.is_empty() { String::new() } else { img(&t) }
                },
                duration: n(x, "/lengthSeconds").map(|d| d as u32),
                views: n(x, "/viewCount"),
                published_ts: ts(&s(x, "/publishedAt")),
                description: Some(s(x, "/game/displayName")).filter(|g| !g.is_empty()),
                ..Default::default()
            })
            .collect())
    }

    /// Angaben für die Video-Seite (Titel, Kanal, Livestatus …)
    pub async fn details(&self, id: &str) -> Result<VideoDetails> {
        let mut parts = id.splitn(3, ':');
        let (_, kind, key) = (parts.next(), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        match kind {
            "live" => {
                let c = self.channel(key).await?;
                let st = c.live.clone();
                Ok(VideoDetails {
                    id: id.to_string(),
                    title: st.as_ref().map(|x| x.title.clone()).unwrap_or_else(|| format!("{} ist offline", c.name)),
                    description: c.description.clone(),
                    author: c.name.clone(),
                    author_id: format!("tw:{}", c.login),
                    author_thumb: Some(c.avatar.clone()),
                    subscribers: c.followers.map(|f| format!("{} Follower", thousands(f))),
                    views_text: st.as_ref().map(|x| viewers_text(x.viewers)),
                    views: st.as_ref().map(|x| x.viewers),
                    live: st.is_some(),
                    category: st.as_ref().map(|x| x.game.clone()).filter(|g| !g.is_empty()),
                    thumb: st.as_ref().map(|x| x.thumb.clone()).unwrap_or_else(|| c.banner.clone().unwrap_or_default()),
                    relative_date: st.as_ref().and_then(|x| x.started).map(|t| format!("live seit {}", since(t).trim_start_matches("vor "))),
                    ..Default::default()
                })
            }
            "vod" => {
                let q = "query($id:ID!){ video(id:$id){ id title description lengthSeconds viewCount publishedAt previewThumbnailURL(width:1280,height:720) game{displayName} owner{ login displayName profileImageURL(width:150) followers{totalCount} } } }";
                let v = self.cached(format!("vod:{key}"), Duration::from_secs(600), q, json!({ "id": key })).await?;
                let x = v.get("video").filter(|x| !x.is_null()).ok_or_else(|| anyhow!("Twitch-Video nicht gefunden (abgelaufen oder nur für Abonnenten)"))?;
                Ok(VideoDetails {
                    id: id.to_string(),
                    title: s(x, "/title"),
                    description: s(x, "/description"),
                    author: s(x, "/owner/displayName"),
                    author_id: format!("tw:{}", s(x, "/owner/login")),
                    author_thumb: Some(img(&s(x, "/owner/profileImageURL"))),
                    subscribers: n(x, "/owner/followers/totalCount").map(|f| format!("{} Follower", thousands(f))),
                    views: n(x, "/viewCount"),
                    views_text: n(x, "/viewCount").map(|v| format!("{} Aufrufe", thousands(v))),
                    duration: n(x, "/lengthSeconds").map(|d| d as u32),
                    category: Some(s(x, "/game/displayName")).filter(|g| !g.is_empty()),
                    thumb: img(&s(x, "/previewThumbnailURL")),
                    relative_date: ts(&s(x, "/publishedAt")).map(since),
                    ..Default::default()
                })
            }
            "clip" => {
                let x = self.clip(key).await?;
                Ok(VideoDetails {
                    id: id.to_string(),
                    title: s(&x, "/title"),
                    author: s(&x, "/broadcaster/displayName"),
                    author_id: format!("tw:{}", s(&x, "/broadcaster/login")),
                    author_thumb: Some(img(&s(&x, "/broadcaster/profileImageURL"))),
                    views: n(&x, "/viewCount"),
                    views_text: n(&x, "/viewCount").map(|v| format!("{} Aufrufe", thousands(v))),
                    duration: x.pointer("/durationSeconds").and_then(Value::as_f64).map(|d| d.round() as u32),
                    category: Some(s(&x, "/game/displayName")).filter(|g| !g.is_empty()),
                    thumb: img(&s(&x, "/thumbnailURL")),
                    relative_date: ts(&s(&x, "/createdAt")).map(since),
                    description: {
                        let by = s(&x, "/curator/displayName");
                        if by.is_empty() { String::new() } else { format!("Clip von {by}") }
                    },
                    ..Default::default()
                })
            }
            _ => Err(anyhow!("Ungültige Twitch-ID")),
        }
    }

    async fn clip(&self, slug: &str) -> Result<Value> {
        let q = "query($s:ID!){ clip(slug:$s){ slug title durationSeconds viewCount createdAt thumbnailURL game{displayName} curator{displayName} broadcaster{ login displayName profileImageURL(width:150) } videoQualities{ quality frameRate sourceURL } playbackAccessToken(params:{platform:\"web\", playerBackend:\"mediaplayer\", playerType:\"site\"}){ value signature } } }";
        let v = self.gql(q, json!({ "s": slug })).await?;
        v.get("clip").filter(|x| !x.is_null()).cloned().ok_or_else(|| anyhow!("Clip nicht gefunden"))
    }

    /// Abspielbare Quelle: HLS-Liste (Live, VOD) oder MP4 (Clip) über den Proxy
    pub async fn streams(&self, id: &str) -> Result<StreamInfo> {
        let mut parts = id.splitn(3, ':');
        let (_, kind, key) = (parts.next(), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let params = json!({ "platform": "web", "playerBackend": "mediaplayer", "playerType": "site" });
        let p: u32 = rand::random::<u32>() % 9_999_999;
        match kind {
            "live" => {
                if !valid_login(key) {
                    return Err(anyhow!("Ungültiger Kanal"));
                }
                let q = "query($l:String!, $p:PlaybackAccessTokenParams!){ streamPlaybackAccessToken(channelName:$l, params:$p){ value signature } }";
                // "popout" bekommt keine eingeblendete Vorab-Werbung ("site" schon)
                // Zum Testen der Werbe-Umgehung lässt sich der Typ setzen (z. B. "site")
                let pt = std::env::var("NRZTUBE_TWITCH_PLAYERTYPE").unwrap_or_else(|_| "popout".into());
                let live_params = json!({ "platform": "web", "playerBackend": "mediaplayer", "playerType": pt });
                let v = self.gql(q, json!({ "l": key.to_lowercase(), "p": live_params })).await?;
                let tok = v.get("streamPlaybackAccessToken").filter(|x| !x.is_null()).ok_or_else(|| anyhow!("Kanal ist offline"))?;
                let url = format!(
                    "https://usher.ttvnw.net/api/channel/hls/{}.m3u8?allow_source=true&fast_bread=true&player_backend=mediaplayer&playlist_include_framerate=true&reassignments_supported=true&supported_codecs=h264&cdm=wv&p={p}&sig={}&token={}",
                    key.to_lowercase(),
                    s(tok, "/signature"),
                    urlencoding::encode(&s(tok, "/value"))
                );
                // Gibt es den Stream? (sonst 404 statt eines leeren Players)
                let res = self.http.get(&url).timeout(Duration::from_secs(10)).send().await?;
                if !res.status().is_success() {
                    return Err(anyhow!("Kanal ist gerade nicht live"));
                }
                let since = self.channel(key).await.ok().and_then(|c| c.live).and_then(|st| st.started);
                Ok(StreamInfo { id: id.to_string(), hls: Some(media(&url)), live: true, live_since: since, ..Default::default() })
            }
            "vod" => {
                if !key.chars().all(|c| c.is_ascii_digit()) {
                    return Err(anyhow!("Ungültige VOD-ID"));
                }
                let q = "query($id:ID!, $p:PlaybackAccessTokenParams!){ videoPlaybackAccessToken(id:$id, params:$p){ value signature } video(id:$id){ lengthSeconds } }";
                let v = self.gql(q, json!({ "id": key, "p": params })).await?;
                let duration = n(&v, "/video/lengthSeconds").map(|d| d as f64);
                let usher = match v.get("videoPlaybackAccessToken").filter(|x| !x.is_null()) {
                    Some(tok) => {
                        let url = format!(
                            "https://usher.ttvnw.net/vod/{key}.m3u8?allow_source=true&player_backend=mediaplayer&playlist_include_framerate=true&supported_codecs=h264&cdm=wv&p={p}&sig={}&token={}",
                            s(tok, "/signature"),
                            urlencoding::encode(&s(tok, "/value"))
                        );
                        let ok = self.http.get(&url).timeout(Duration::from_secs(10)).send().await.map(|r| r.status().is_success()).unwrap_or(false);
                        ok.then_some(url)
                    }
                    None => None,
                };
                let hls = match usher {
                    Some(url) => media(&url),
                    // Nur für Abonnenten (oder Usher verweigert): direkt von CloudFront (wie TwitchNoSub)
                    None => {
                        self.nosub_master(key).await.map_err(|_| anyhow!("VOD nicht verfügbar (abgelaufen oder gelöscht)"))?;
                        format!("/api/twitch/nosub/{key}")
                    }
                };
                Ok(StreamInfo { id: id.to_string(), hls: Some(hls), duration, ..Default::default() })
            }
            "clip" => {
                let x = self.clip(key).await?;
                let sig = s(&x, "/playbackAccessToken/signature");
                let tok = urlencoding::encode(&s(&x, "/playbackAccessToken/value")).into_owned();
                let formats: Vec<Format> = x
                    .pointer("/videoQualities")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|q| {
                        let h: u32 = s(q, "/quality").parse().unwrap_or(0);
                        let fps = q.pointer("/frameRate").and_then(Value::as_f64).unwrap_or(30.0).round() as u32;
                        let src = s(q, "/sourceURL");
                        let sep = if src.contains('?') { '&' } else { '?' };
                        Format {
                            itag: format!("clip{h}"),
                            label: if fps > 30 { format!("{h}p{fps}") } else { format!("{h}p") },
                            ext: "mp4".into(),
                            kind: "av".into(),
                            codec: "avc1".into(),
                            height: h,
                            fps,
                            bitrate: 0,
                            size: None,
                            url: media(&format!("{src}{sep}sig={sig}&token={tok}")),
                        }
                    })
                    .collect();
                if formats.is_empty() {
                    return Err(anyhow!("Clip lässt sich nicht abspielen"));
                }
                Ok(StreamInfo {
                    id: id.to_string(),
                    formats,
                    duration: x.pointer("/durationSeconds").and_then(Value::as_f64),
                    ..Default::default()
                })
            }
            _ => Err(anyhow!("Ungültige Twitch-ID")),
        }
    }
}

/// Qualitäten der VOD-Ablage: (Ordner, Auflösung, Bildrate)
const NOSUB_QUALITIES: &[(&str, &str, u32)] = &[
    ("chunked", "", 60),
    ("1440p60", "2560x1440", 60),
    ("1080p60", "1920x1080", 60),
    ("1080p30", "1920x1080", 30),
    ("720p60", "1280x720", 60),
    ("720p30", "1280x720", 30),
    ("480p30", "854x480", 30),
    ("360p30", "640x360", 30),
    ("160p30", "284x160", 30),
];

impl Twitch {
    /// Hauptliste eines VODs direkt aus der CloudFront-Ablage – auch für VODs, die nur für
    /// Abonnenten freigegeben sind (Methode von TwitchNoSub). Adressen noch nicht über den Proxy.
    pub async fn nosub_master(&self, vod: &str) -> Result<String> {
        let key = format!("nosub:{vod}");
        if let Some((t, v)) = self.cache.lock().await.get(&key) {
            if t.elapsed() < Duration::from_secs(3600) {
                if let Some(m) = v.as_str() {
                    return Ok(m.to_string());
                }
            }
        }
        let q = "query($id:ID!){ video(id:$id){ broadcastType createdAt seekPreviewsURL owner{ login } } }";
        let v = self.gql(q, json!({ "id": vod })).await?;
        let x = v.get("video").filter(|x| !x.is_null()).ok_or_else(|| anyhow!("VOD nicht gefunden"))?;
        let preview = s(x, "/seekPreviewsURL");
        let rest = preview.strip_prefix("https://").ok_or_else(|| anyhow!("VOD ohne Ablage-Adresse"))?;
        let (domain, path) = rest.split_once('/').ok_or_else(|| anyhow!("VOD ohne Ablage-Adresse"))?;
        let parts: Vec<&str> = path.split('/').collect();
        let special = parts.iter().position(|p| p.contains("storyboards")).and_then(|i| i.checked_sub(1)).map(|i| parts[i]).ok_or_else(|| anyhow!("VOD-Kennung nicht gefunden"))?;
        let kind = s(x, "/broadcastType").to_lowercase();
        let old_upload = kind == "upload" && ts(&s(x, "/createdAt")).is_some_and(|t| t < 1_676_000_000);
        let login = s(x, "/owner/login");
        let urls: Vec<(usize, String)> = NOSUB_QUALITIES
            .iter()
            .enumerate()
            .map(|(i, (q, _, _))| {
                let u = if kind == "highlight" {
                    format!("https://{domain}/{special}/{q}/highlight-{vod}.m3u8")
                } else if old_upload {
                    format!("https://{domain}/{login}/{vod}/{special}/{q}/index-dvr.m3u8")
                } else {
                    format!("https://{domain}/{special}/{q}/index-dvr.m3u8")
                };
                (i, u)
            })
            .collect();
        // Alle Qualitäten gleichzeitig prüfen
        let checks = futures::future::join_all(urls.iter().map(|(i, u)| async move {
            let text = self.fetch_text(u).await?;
            if !text.contains("#EXTINF") {
                return None;
            }
            // fMP4 kann HEVC/AV1 sein (spielt der Browser meist nicht): am Init-Segment prüfen
            if text.contains("init-0.mp4") {
                let init = self.http.get(u.replace(&u[u.rfind('/').unwrap_or(0) + 1..], "init-0.mp4")).timeout(Duration::from_secs(4)).send().await.ok()?.bytes().await.ok()?;
                let w = |m: &[u8]| init.windows(4).any(|x| x == m);
                if w(b"hev1") || w(b"hvc1") || w(b"av01") {
                    return None;
                }
            }
            Some(*i)
        }))
        .await;
        let mut out = String::from("#EXTM3U\n");
        let mut bw = 9_000_000u32;
        for i in checks.into_iter().flatten() {
            let (q, res, fps) = NOSUB_QUALITIES[i];
            let res_attr = if res.is_empty() { String::new() } else { format!(",RESOLUTION={res}") };
            out.push_str(&format!("#EXT-X-STREAM-INF:BANDWIDTH={bw},CODECS=\"avc1.64002A,mp4a.40.2\"{res_attr},FRAME-RATE={fps},NAME=\"{q}\"\n{}\n", urls[i].1));
            bw = bw.saturating_sub(900_000).max(200_000);
        }
        if !out.contains("STREAM-INF") {
            return Err(anyhow!("Keine abspielbare Qualität gefunden"));
        }
        self.cache.lock().await.insert(key, (Instant::now(), Value::String(out.clone())));
        Ok(out)
    }
}

/// Livestream als Listeneintrag
pub fn live_item(login: &str, name: &str, avatar: &str, st: &TwitchStream) -> VideoItem {
    VideoItem {
        id: format!("tw:live:{}", login.to_lowercase()),
        title: st.title.clone(),
        author: name.to_string(),
        author_id: format!("tw:{}", login.to_lowercase()),
        author_thumb: Some(avatar.to_string()).filter(|a| !a.is_empty()),
        // Live-Vorschaubilder ändern sich alle paar Minuten: Zeitstempel gegen den Cache
        thumb: if st.thumb.is_empty() { String::new() } else { format!("{}?t={}", st.thumb, crate::db::now() / 300) },
        views: Some(st.viewers),
        views_text: Some(viewers_text(st.viewers)),
        published_ts: st.started,
        live: true,
        description: Some(st.game.clone()).filter(|g| !g.is_empty()),
        ..Default::default()
    }
}

fn since(t: i64) -> String {
    let d = (crate::db::now() - t).max(0);
    let (n, unit) = match d {
        d if d < 3600 => ((d / 60).max(1), if d / 60 == 1 { "Minute" } else { "Minuten" }),
        d if d < 86400 => (d / 3600, if d / 3600 == 1 { "Stunde" } else { "Stunden" }),
        d if d < 86400 * 30 => (d / 86400, if d / 86400 == 1 { "Tag" } else { "Tagen" }),
        d if d < 86400 * 365 => (d / (86400 * 30), if d / (86400 * 30) == 1 { "Monat" } else { "Monaten" }),
        d => (d / (86400 * 365), if d / (86400 * 365) == 1 { "Jahr" } else { "Jahren" }),
    };
    format!("vor {n} {unit}")
}

// ------------------------------------------------------------------ Werbung in Livestreams

/// Werbung in einer Live-Medienliste? (eingeblendete Abschnitte heissen nicht "live")
pub fn has_ads(text: &str) -> bool {
    text.contains("stitched-ad")
        || text.lines().any(|l| {
            l.strip_prefix("#EXTINF:").and_then(|r| r.split_once(',')).map(|(_, t)| {
                let t = t.trim();
                !t.is_empty() && t != "live"
            }) == Some(true)
        })
}

/// Kanal aus einer Usher-Hauptliste (…/api/channel/hls/<login>.m3u8)
pub fn usher_channel(url: &str) -> Option<String> {
    let rest = url.split("/api/channel/hls/").nth(1)?;
    let login = rest.split(".m3u8").next()?.to_lowercase();
    valid_login(&login).then_some(login)
}

/// In einer (schon umgeschriebenen) Hauptliste jede Variante mit Kanal und Auflösung versehen
pub fn tag_variants(text: &str, login: &str) -> String {
    let mut out = String::with_capacity(text.len() + 512);
    let mut res = String::new();
    for line in text.lines() {
        if line.starts_with("#EXT-X-STREAM-INF:") {
            res = line.split("RESOLUTION=").nth(1).and_then(|r| r.split(',').next()).unwrap_or("").to_string();
        } else if line.starts_with("/p/") {
            let sep = if line.contains('?') { '&' } else { '?' };
            out.push_str(&format!("{line}{sep}tw={login}&res={}\n", urlencoding::encode(&res)));
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Player-Typen für Ersatzlisten (wie TwitchAdSolutions/vaft): erst volle Qualität, dann 360p
const BACKUP_TYPES: &[&str] = &["embed", "mobile_web", "frontpage", "site", "autoplay", "picture-by-picture"];

impl Twitch {
    /// Werbefreie Medienliste derselben Auflösung über einen anderen Player-Typ.
    /// Merkt sich pro Kanal, welcher Typ zuletzt sauber war (10 Minuten).
    pub async fn backup_media(&self, login: &str, res: &str) -> Option<(String, String)> {
        let started = Instant::now();
        // 1. Gemerkte Ersatz-Variante wiederverwenden
        let pinned = self.backups.lock().await.get(login).filter(|(t, _, _)| t.elapsed() < Duration::from_secs(600)).cloned();
        if let Some((_, ty, variants)) = &pinned {
            if let Some(u) = pick_variant(variants, res) {
                if let Some(t) = self.fetch_text(&u).await.filter(|t| !has_ads(t)) {
                    return Some((u, t));
                }
            }
            tracing::debug!("Twitch: Ersatz {ty} für {login} hat jetzt auch Werbung");
        }
        // 2. Player-Typen der Reihe nach (gemerkter zuerst überspringen, der hatte gerade Werbung)
        let skip = pinned.map(|(_, ty, _)| ty);
        for ty in BACKUP_TYPES.iter().filter(|t| Some(t.to_string()) != skip) {
            if started.elapsed() > Duration::from_secs(6) {
                break;
            }
            let Some(variants) = self.usher_variants(login, ty).await else { continue };
            let Some(u) = pick_variant(&variants, res) else { continue };
            if let Some(t) = self.fetch_text(&u).await.filter(|t| !has_ads(t)) {
                self.backups.lock().await.insert(login.to_string(), (Instant::now(), ty.to_string(), variants));
                tracing::debug!("Twitch: Werbung umgangen mit {ty} ({login}, {res})");
                return Some((u, t));
            }
        }
        self.backups.lock().await.remove(login);
        None
    }

    async fn fetch_text(&self, url: &str) -> Option<String> {
        let r = self.http.get(url).timeout(Duration::from_secs(4)).send().await.ok()?;
        if !r.status().is_success() {
            return None;
        }
        r.text().await.ok()
    }

    /// Varianten (Auflösung -> Adresse) einer Hauptliste mit dem angegebenen Player-Typ
    async fn usher_variants(&self, login: &str, player_type: &str) -> Option<Vec<(String, String)>> {
        let q = "query($l:String!, $p:PlaybackAccessTokenParams!){ streamPlaybackAccessToken(channelName:$l, params:$p){ value signature } }";
        let params = json!({ "platform": "web", "playerBackend": "mediaplayer", "playerType": player_type });
        let v = self.gql(q, json!({ "l": login, "p": params })).await.ok()?;
        let tok = v.get("streamPlaybackAccessToken").filter(|x| !x.is_null())?;
        let p: u32 = rand::random::<u32>() % 9_999_999;
        let url = format!(
            "https://usher.ttvnw.net/api/channel/hls/{login}.m3u8?allow_source=true&fast_bread=true&player_backend=mediaplayer&playlist_include_framerate=true&supported_codecs=h264&cdm=wv&p={p}&sig={}&token={}",
            s(tok, "/signature"),
            urlencoding::encode(&s(tok, "/value"))
        );
        let master = self.fetch_text(&url).await?;
        let mut out = vec![];
        let mut res = String::new();
        for line in master.lines() {
            if line.starts_with("#EXT-X-STREAM-INF:") {
                res = line.split("RESOLUTION=").nth(1).and_then(|r| r.split(',').next()).unwrap_or("").to_string();
            } else if line.starts_with("http") {
                out.push((res.clone(), line.trim().to_string()));
            }
        }
        (!out.is_empty()).then_some(out)
    }
}

/// Gleiche Auflösung, sonst die nächstkleinere, sonst die erste
fn pick_variant(variants: &[(String, String)], res: &str) -> Option<String> {
    if let Some((_, u)) = variants.iter().find(|(r, _)| r == res) {
        return Some(u.clone());
    }
    let h = |r: &str| r.split('x').nth(1).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    let want = h(res);
    variants
        .iter()
        .filter(|(r, _)| want == 0 || h(r) <= want)
        .max_by_key(|(r, _)| h(r))
        .or(variants.first())
        .map(|(_, u)| u.clone())
}


/// Entfernt eingeblendete Werbeblöcke aus einer Twitch-Live-Medienliste: Echte Segmente
/// heissen `#EXTINF:2.000,live`, Werbung trägt einen anderen Titel (wie bei streamlink).
pub fn strip_ads(text: &str) -> String {
    if !text.contains(",live") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut skip_uri = false;
    for line in text.lines() {
        if skip_uri {
            if !line.starts_with('#') && !line.trim().is_empty() {
                skip_uri = false;
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let title = rest.split_once(',').map(|(_, t)| t.trim()).unwrap_or("");
            if !title.is_empty() && title != "live" {
                skip_uri = true;
                continue;
            }
        }
        if line.starts_with("#EXT-X-DATERANGE") && line.contains("stitched-ad") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

// ------------------------------------------------------------------ Chat

type Tls = tokio_rustls::client::TlsStream<tokio::net::TcpStream>;

/// TLS-Verbindung zum Twitch-Chat (Port 6697)
async fn irc_connect() -> Result<Tls> {
    use std::sync::OnceLock;
    static CFG: OnceLock<Arc<tokio_rustls::rustls::ClientConfig>> = OnceLock::new();
    let cfg = CFG
        .get_or_init(|| {
            let mut roots = tokio_rustls::rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            Arc::new(tokio_rustls::rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth())
        })
        .clone();
    let tcp = tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect("irc.chat.twitch.tv:6697")).await??;
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from("irc.chat.twitch.tv")?;
    Ok(tokio_rustls::TlsConnector::from(cfg).connect(name, tcp).await?)
}

/// Anonym (justinfan) lesen und Zeilen weiterreichen, bis der Empfänger weg ist
pub async fn chat(tw: Arc<Twitch>, login: String, tx: tokio::sync::mpsc::Sender<ChatLine>) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let login = login.to_lowercase();
    // Emotes von 7TV, BTTV und FFZ parallel zur Verbindung laden
    let emotes_job = {
        let tw = tw.clone();
        let l = login.clone();
        tokio::spawn(async move { tw.third_party_emotes(&l).await })
    };
    let stream = irc_connect().await?;
    let (r, mut w) = tokio::io::split(stream);
    let nick = format!("justinfan{}", 10000 + rand::random::<u32>() % 80000);
    w.write_all(format!("CAP REQ :twitch.tv/tags\r\nPASS SCHMOOPIIE\r\nNICK {nick}\r\nJOIN #{login}\r\n").as_bytes()).await?;
    let emotes = emotes_job.await.unwrap_or_default();
    let mut lines = BufReader::new(r).lines();
    loop {
        let line = tokio::select! {
            l = lines.next_line() => match l? { Some(l) => l, None => return Ok(()) },
            _ = tx.closed() => return Ok(()),
        };
        if line.starts_with("PING") {
            w.write_all(line.replacen("PING", "PONG", 1).as_bytes()).await?;
            w.write_all(b"\r\n").await?;
            continue;
        }
        if let Some(msg) = parse_privmsg(&line, &emotes) {
            if tx.send(msg).await.is_err() {
                return Ok(());
            }
        }
    }
}

fn parse_privmsg(line: &str, extra: &HashMap<String, String>) -> Option<ChatLine> {
    let (tags, rest) = match line.strip_prefix('@') {
        Some(l) => l.split_once(' ')?,
        None => ("", line),
    };
    let (_, after) = rest.split_once(" PRIVMSG #")?;
    let (_, text) = after.split_once(" :")?;
    let tags: HashMap<&str, &str> = tags.split(';').filter_map(|t| t.split_once('=')).collect();
    let user = tags.get("display-name").copied().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| {
        rest.trim_start_matches(':').split('!').next().unwrap_or("").to_string()
    });
    let text = text.strip_prefix("\u{1}ACTION ").map(|t| t.trim_end_matches('\u{1}')).unwrap_or(text);
    // Twitch-Emotes: "25:0-4,12-16/1902:6-10" (Positionen in Zeichen, nicht Bytes)
    let mut ranges: Vec<(usize, usize, String)> = vec![];
    if let Some(e) = tags.get("emotes").filter(|e| !e.is_empty()) {
        for em in e.split('/') {
            let Some((id, pos)) = em.split_once(':') else { continue };
            for p in pos.split(',') {
                if let Some((a, b)) = p.split_once('-') {
                    if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                        ranges.push((a, b, id.to_string()));
                    }
                }
            }
        }
    }
    ranges.sort();
    let chars: Vec<char> = text.chars().collect();
    let mut parts = vec![];
    let mut i = 0;
    for (a, b, id) in ranges {
        if a < i || b >= chars.len() {
            continue;
        }
        if a > i {
            push_text(&mut parts, &chars[i..a].iter().collect::<String>(), extra);
        }
        let name: String = chars[a..=b].iter().collect();
        parts.push(ChatPart::Emote { name, url: img(&format!("https://static-cdn.jtvnw.net/emoticons/v2/{id}/default/dark/1.0")) });
        i = b + 1;
    }
    if i < chars.len() {
        push_text(&mut parts, &chars[i..].iter().collect::<String>(), extra);
    }
    Some(ChatLine {
        id: tags.get("id").map(|s| s.to_string()).unwrap_or_default(),
        user,
        color: tags.get("color").map(|s| s.to_string()).unwrap_or_default(),
        parts,
    })
}

/// Text in Wörter teilen und 7TV/BTTV/FFZ-Emotes ersetzen
fn push_text(parts: &mut Vec<ChatPart>, text: &str, extra: &HashMap<String, String>) {
    if extra.is_empty() {
        parts.push(ChatPart::Text { t: text.to_string() });
        return;
    }
    let mut buf = String::new();
    for (k, word) in text.split(' ').enumerate() {
        if k > 0 {
            buf.push(' ');
        }
        match extra.get(word) {
            Some(url) => {
                if !buf.is_empty() {
                    parts.push(ChatPart::Text { t: std::mem::take(&mut buf) });
                }
                parts.push(ChatPart::Emote { name: word.to_string(), url: url.clone() });
            }
            None => buf.push_str(word),
        }
    }
    if !buf.is_empty() {
        parts.push(ChatPart::Text { t: buf });
    }
}

// ------------------------------------------------------------------ Emotes von 7TV, BTTV, FFZ

impl Twitch {
    async fn get_json(&self, url: &str) -> Option<Value> {
        let r = self.http.get(url).timeout(Duration::from_secs(6)).send().await.ok()?;
        if !r.status().is_success() {
            return None;
        }
        r.json().await.ok()
    }

    async fn user_id(&self, login: &str) -> Option<String> {
        let v = self.cached(format!("uid:{login}"), Duration::from_secs(86400), "query($l:String!){ user(login:$l){ id } }", json!({ "l": login })).await.ok()?;
        v.pointer("/user/id").and_then(Value::as_str).map(str::to_string)
    }

    /// Name -> Bild (über den Proxy). Kanal-Emotes überschreiben globale.
    pub async fn third_party_emotes(&self, login: &str) -> HashMap<String, String> {
        let key = format!("emotes:{login}");
        if let Some((t, v)) = self.cache.lock().await.get(&key) {
            if t.elapsed() < Duration::from_secs(1800) {
                if let Ok(m) = serde_json::from_value::<HashMap<String, String>>(v.clone()) {
                    return m;
                }
            }
        }
        let uid = self.user_id(login).await.unwrap_or_default();
        let (u_stv, u_bttv, u_ffz) = (
            format!("https://7tv.io/v3/users/twitch/{uid}"),
            format!("https://api.betterttv.net/3/cached/users/twitch/{uid}"),
            format!("https://api.frankerfacez.com/v1/room/id/{uid}"),
        );
        let (stv_g, stv_u, bttv_g, bttv_u, ffz_g, ffz_u) = tokio::join!(
            self.get_json("https://7tv.io/v3/emote-sets/global"),
            self.get_json(&u_stv),
            self.get_json("https://api.betterttv.net/3/cached/emotes/global"),
            self.get_json(&u_bttv),
            self.get_json("https://api.frankerfacez.com/v1/set/global"),
            self.get_json(&u_ffz),
        );
        let mut m: HashMap<String, String> = HashMap::new();
        let ffz = |v: &Option<Value>, m: &mut HashMap<String, String>| {
            for set in v.as_ref().and_then(|v| v.get("sets")).and_then(Value::as_object).map(|o| o.values().collect::<Vec<_>>()).unwrap_or_default() {
                for e in set.get("emoticons").and_then(Value::as_array).into_iter().flatten() {
                    let name = s(e, "/name");
                    let u = e.pointer("/urls/1").and_then(Value::as_str).unwrap_or("");
                    if !name.is_empty() && !u.is_empty() {
                        let u = if u.starts_with("//") { format!("https:{u}") } else { u.to_string() };
                        m.insert(name, img(&u));
                    }
                }
            }
        };
        let bttv = |list: Option<&Vec<Value>>, m: &mut HashMap<String, String>| {
            for e in list.into_iter().flatten() {
                let (id, code) = (s(e, "/id"), s(e, "/code"));
                if !id.is_empty() && !code.is_empty() {
                    m.insert(code, img(&format!("https://cdn.betterttv.net/emote/{id}/1x")));
                }
            }
        };
        let stv = |list: Option<&Vec<Value>>, m: &mut HashMap<String, String>| {
            for e in list.into_iter().flatten() {
                let (id, name) = (s(e, "/id"), s(e, "/name"));
                if !id.is_empty() && !name.is_empty() {
                    m.insert(name, img(&format!("https://cdn.7tv.app/emote/{id}/1x.webp")));
                }
            }
        };
        // Reihenfolge: global zuerst, dann Kanal (überschreibt)
        ffz(&ffz_g, &mut m);
        bttv(bttv_g.as_ref().and_then(Value::as_array), &mut m);
        stv(stv_g.as_ref().and_then(|v| v.get("emotes")).and_then(Value::as_array), &mut m);
        ffz(&ffz_u, &mut m);
        bttv(bttv_u.as_ref().and_then(|v| v.get("channelEmotes")).and_then(Value::as_array), &mut m);
        bttv(bttv_u.as_ref().and_then(|v| v.get("sharedEmotes")).and_then(Value::as_array), &mut m);
        stv(stv_u.as_ref().and_then(|v| v.pointer("/emote_set/emotes")).and_then(Value::as_array), &mut m);
        // Sehr kurze "Emotes" (z. B. "a") würden normale Wörter ersetzen
        m.retain(|k, _| k.chars().count() >= 2);
        if let Ok(v) = serde_json::to_value(&m) {
            self.cache.lock().await.insert(key, (Instant::now(), v));
        }
        m
    }
}

// ------------------------------------------------------------------ Anmeldung (Geräte-Code) und Schreiben

/// Client-ID der Twitch-App für Fernseher: erlaubt die Anmeldung per Code auf twitch.tv/activate
const TV_CLIENT: &str = "ue6666qo983tsx6so1t0vnawi233wa";
const SCOPES: &str = "chat:read chat:edit user:read:follows";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TwAuth {
    pub access: String,
    pub refresh: String,
    pub expires: i64,
    pub login: String,
    pub user_id: String,
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={}", urlencoding::encode(v))).collect::<Vec<_>>().join("&")
}

impl Twitch {
    async fn post_form(&self, url: &str, pairs: &[(&str, &str)]) -> Result<(u16, Value)> {
        let r = self
            .http
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form(pairs))
            .timeout(Duration::from_secs(12))
            .send()
            .await?;
        let st = r.status().as_u16();
        Ok((st, r.json().await.unwrap_or(Value::Null)))
    }

    /// Schritt 1: Code holen, den man auf twitch.tv/activate eingibt
    pub async fn device_start(&self) -> Result<Value> {
        let (st, v) = self.post_form("https://id.twitch.tv/oauth2/device", &[("client_id", TV_CLIENT), ("scopes", SCOPES)]).await?;
        if st != 200 {
            return Err(anyhow!("Twitch-Anmeldung nicht möglich ({})", s(&v, "/message")));
        }
        Ok(v)
    }

    /// Schritt 2: nachfragen, ob bestätigt wurde. Ok(None) = noch nicht.
    pub async fn device_poll(&self, device_code: &str) -> Result<Option<TwAuth>> {
        let (st, v) = self
            .post_form(
                "https://id.twitch.tv/oauth2/token",
                &[("client_id", TV_CLIENT), ("scopes", SCOPES), ("device_code", device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")],
            )
            .await?;
        if st != 200 {
            let msg = s(&v, "/message");
            if msg.contains("pending") || msg.contains("slow_down") {
                return Ok(None);
            }
            return Err(anyhow!(if msg.contains("expired") || msg.contains("invalid device code") { "Der Code ist abgelaufen – bitte neu anmelden".to_string() } else { format!("Twitch-Anmeldung abgelehnt ({msg})") }));
        }
        let access = s(&v, "/access_token");
        let (login, user_id) = self.validate(&access).await?;
        Ok(Some(TwAuth {
            access,
            refresh: s(&v, "/refresh_token"),
            expires: crate::db::now() + v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600),
            login,
            user_id,
        }))
    }

    async fn validate(&self, token: &str) -> Result<(String, String)> {
        let r = self.http.get("https://id.twitch.tv/oauth2/validate").header("Authorization", format!("OAuth {token}")).send().await?;
        if !r.status().is_success() {
            return Err(anyhow!("Twitch-Anmeldung ungültig"));
        }
        let v: Value = r.json().await?;
        Ok((s(&v, "/login"), s(&v, "/user_id")))
    }

    /// Token erneuern, wenn er bald abläuft. Gibt `Some` zurück, wenn sich etwas geändert hat.
    pub async fn fresh(&self, a: &TwAuth) -> Result<Option<TwAuth>> {
        if a.expires > crate::db::now() + 120 {
            return Ok(None);
        }
        let (st, v) = self
            .post_form("https://id.twitch.tv/oauth2/token", &[("client_id", TV_CLIENT), ("grant_type", "refresh_token"), ("refresh_token", &a.refresh)])
            .await?;
        if st != 200 {
            return Err(anyhow!("Twitch-Anmeldung abgelaufen – bitte in den Einstellungen neu verbinden"));
        }
        Ok(Some(TwAuth {
            access: s(&v, "/access_token"),
            refresh: Some(s(&v, "/refresh_token")).filter(|r| !r.is_empty()).unwrap_or_else(|| a.refresh.clone()),
            expires: crate::db::now() + v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600),
            login: a.login.clone(),
            user_id: a.user_id.clone(),
        }))
    }

    /// Kanäle, denen das Twitch-Konto folgt (login, Anzeigename)
    pub async fn followed(&self, a: &TwAuth) -> Result<Vec<(String, String)>> {
        let mut out = vec![];
        let mut cursor = String::new();
        for _ in 0..20 {
            let mut url = format!("https://api.twitch.tv/helix/channels/followed?user_id={}&first=100", a.user_id);
            if !cursor.is_empty() {
                url.push_str(&format!("&after={cursor}"));
            }
            let r = self.http.get(&url).header("Client-Id", TV_CLIENT).header("Authorization", format!("Bearer {}", a.access)).send().await?;
            if !r.status().is_success() {
                return Err(anyhow!("Twitch: gefolgte Kanäle nicht abrufbar ({})", r.status()));
            }
            let v: Value = r.json().await?;
            for d in v.get("data").and_then(Value::as_array).into_iter().flatten() {
                out.push((s(d, "/broadcaster_login"), s(d, "/broadcaster_name")));
            }
            cursor = s(&v, "/pagination/cursor");
            if cursor.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    /// Nachricht in einen Chat schreiben (eine Verbindung pro Konto, bleibt eine Weile offen)
    pub async fn send_chat(&self, user: i64, a: &TwAuth, channel: &str, text: &str) -> Result<()> {
        let text: String = text.replace(['\r', '\n'], " ").trim().chars().take(500).collect();
        if text.is_empty() {
            return Err(anyhow!("Leere Nachricht"));
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        let req = WriteReq { channel: channel.to_lowercase(), text, reply: tx };
        let sender = {
            let mut w = self.writers.lock().await;
            let reuse = w.get(&user).filter(|(tok, s)| tok == &a.access && !s.is_closed()).map(|(_, s)| s.clone());
            match reuse {
                Some(s) => s,
                None => {
                    let (stx, srx) = tokio::sync::mpsc::channel::<WriteReq>(16);
                    let (tok, login) = (a.access.clone(), a.login.clone());
                    tokio::spawn(async move {
                        if let Err(e) = writer_task(tok, login, srx).await {
                            tracing::debug!("Twitch-Chat schreiben: {e}");
                        }
                    });
                    w.insert(user, (a.access.clone(), stx.clone()));
                    stx
                }
            }
        };
        sender.send(req).await.map_err(|_| anyhow!("Chat-Verbindung getrennt – bitte nochmal senden"))?;
        match tokio::time::timeout(Duration::from_secs(15), rx).await {
            Ok(Ok(r)) => r.map_err(|e| anyhow!(e)),
            _ => Err(anyhow!("Twitch antwortet nicht")),
        }
    }
}

pub struct WriteReq {
    channel: String,
    text: String,
    reply: tokio::sync::oneshot::Sender<std::result::Result<(), String>>,
}

/// Twitch-Hinweise (NOTICE msg-id) auf Deutsch
fn notice_text(id: &str, fallback: &str) -> String {
    match id {
        "msg_followersonly" | "msg_followersonly_followed" | "msg_followersonly_zero" => "Nur Follower dürfen hier schreiben (evtl. erst nach einer Wartezeit).".into(),
        "msg_subsonly" => "Nur Abonnenten dürfen hier schreiben.".into(),
        "msg_emoteonly" => "Gerade sind nur Emotes erlaubt.".into(),
        "msg_slowmode" => "Langsamer Modus: bitte kurz warten.".into(),
        "msg_ratelimit" => "Zu viele Nachrichten – bitte kurz warten.".into(),
        "msg_duplicate" => "Die gleiche Nachricht wurde gerade schon gesendet.".into(),
        "msg_banned" => "Du bist in diesem Chat gesperrt.".into(),
        "msg_timedout" => "Du bist in diesem Chat vorübergehend stummgeschaltet.".into(),
        "msg_verified_email" => "Twitch verlangt eine bestätigte E-Mail-Adresse.".into(),
        "msg_requires_verified_phone_number" => "Twitch verlangt eine bestätigte Telefonnummer.".into(),
        "msg_channel_suspended" => "Dieser Kanal ist gesperrt.".into(),
        _ => if fallback.is_empty() { format!("Twitch: {id}") } else { format!("Twitch: {fallback}") },
    }
}

async fn writer_task(token: String, login: String, mut rx: tokio::sync::mpsc::Receiver<WriteReq>) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stream = irc_connect().await?;
    let (r, mut w) = tokio::io::split(stream);
    w.write_all(format!("CAP REQ :twitch.tv/tags twitch.tv/commands\r\nPASS oauth:{token}\r\nNICK {login}\r\n").as_bytes()).await?;
    let mut lines = BufReader::new(r).lines();
    // Auf die Begrüssung (001) oder einen Anmeldefehler warten
    let mut login_err: Option<String> = None;
    let welcome = tokio::time::timeout(Duration::from_secs(10), async {
        while let Ok(Some(l)) = lines.next_line().await {
            if l.contains(" 001 ") {
                return true;
            }
            if l.contains("NOTICE * :") {
                login_err = Some(l.split(" :").last().unwrap_or("").to_string());
                return false;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    let mut joined: std::collections::HashSet<String> = Default::default();
    if !welcome {
        let msg = format!("Twitch-Anmeldung fehlgeschlagen{} – bitte in den Einstellungen neu verbinden", login_err.map(|e| format!(" ({e})")).unwrap_or_default());
        while let Ok(Some(req)) = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
            let _ = req.reply.send(Err(msg.clone()));
        }
        return Err(anyhow!(msg));
    }
    loop {
        let req = tokio::select! {
            r = rx.recv() => match r { Some(r) => r, None => return Ok(()) },
            l = lines.next_line() => {
                let Some(l) = l? else { return Ok(()) };
                if l.starts_with("PING") {
                    w.write_all(l.replacen("PING", "PONG", 1).as_bytes()).await?;
                    w.write_all(b"\r\n").await?;
                }
                continue;
            }
            _ = tokio::time::sleep(Duration::from_secs(900)) => return Ok(()),
        };
        if joined.insert(req.channel.clone()) {
            w.write_all(format!("JOIN #{}\r\n", req.channel).as_bytes()).await?;
        }
        w.write_all(format!("PRIVMSG #{} :{}\r\n", req.channel, req.text).as_bytes()).await?;
        // Antwort abwarten: USERSTATE = angekommen, NOTICE = abgelehnt
        let chan = format!("#{}", req.channel);
        let res = tokio::time::timeout(Duration::from_millis(2500), async {
            while let Ok(Some(l)) = lines.next_line().await {
                if l.starts_with("PING") {
                    let _ = w.write_all(format!("{}\r\n", l.replacen("PING", "PONG", 1)).as_bytes()).await;
                    continue;
                }
                if l.contains(&format!(" NOTICE {chan} ")) {
                    let id = l.strip_prefix("@msg-id=").and_then(|x| x.split([' ', ';']).next()).unwrap_or("");
                    let fallback = l.split(" :").last().unwrap_or("");
                    return Err(notice_text(id, fallback));
                }
                if l.contains(&format!(" USERSTATE {chan}")) && !l.contains("JOIN") {
                    return Ok(());
                }
            }
            Err("Verbindung getrennt".to_string())
        })
        .await
        // Keine Rückmeldung: meist trotzdem angekommen
        .unwrap_or(Ok(()));
        let _ = req.reply.send(res);
    }
}

// ------------------------------------------------------------------ Verknüpfung YouTube <-> Twitch

/// Twitch-Logins aus Text oder Adressen (twitch.tv/<login>)
pub fn twitch_logins_in(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out: Vec<String> = vec![];
    for (i, _) in lower.match_indices("twitch.tv/") {
        let login: String = lower[i + 10..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let reserved = ["videos", "directory", "p", "settings", "downloads", "jobs", "search", "subs", "turbo", "prime", "login", "signup", "store", "team", "inventory", "wallet", "drops"];
        if valid_login(&login) && login.len() >= 3 && !reserved.contains(&login.as_str()) && !out.contains(&login) {
            out.push(login);
        }
    }
    out
}

/// Name zum Vergleichen: klein, nur Buchstaben und Ziffern
pub fn norm(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Gleich genug? (MontanaBlack == montanablack88, "Papaplatte" == papaplatte)
pub fn similar(a: &str, b: &str) -> bool {
    let (a, b) = (norm(a), norm(b));
    if a.len() < 4 || b.len() < 4 {
        return a == b && !a.is_empty();
    }
    if a == b {
        return true;
    }
    let strip = |s: &str| s.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches("tv").trim_end_matches("live").trim_end_matches("official").to_string();
    let (sa, sb) = (strip(&a), strip(&b));
    sa.len() >= 4 && sa == sb
}

/// YouTube-Adressen in den Social-Links eines Twitch-Kanals
pub fn youtube_urls(socials: &[(String, String)], description: &str) -> Vec<String> {
    let mut out: Vec<String> = socials.iter().map(|(_, u)| u.clone()).filter(|u| u.contains("youtube.com/") || u.contains("youtu.be/")).collect();
    for word in description.split_whitespace() {
        if word.contains("youtube.com/") && !out.iter().any(|u| u == word) {
            let u = if word.starts_with("http") { word.to_string() } else { format!("https://{word}") };
            out.push(u.trim_end_matches(['.', ',', ')', '!']).to_string());
        }
    }
    out.retain(|u| !u.contains("watch?v=") && !u.contains("youtu.be/"));
    out
}
