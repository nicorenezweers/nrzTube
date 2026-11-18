//! Eigener YouTube-Client über die interne InnerTube-API (wie Invidious und FreeTube).
//! Alle Anfragen gehen vom Server aus – ohne Cookies, ohne Konto, ohne Werbung.

pub mod json;
pub mod parse;

use anyhow::{anyhow, Result};
use base64::Engine;
use json::*;
use nrztube_types::*;
use parse::*;
use serde_json::{json, Value};
use std::time::Duration;

pub(crate) const BASE: &str = "https://www.youtube.com/youtubei/v1";
pub(crate) const CLIENT_VERSION: &str = "2.20261101.00.00";
pub(crate) const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

#[derive(Clone)]
pub struct Yt {
    http: reqwest::Client,
    /// Video-Details (Titel, Beschreibung, ähnliche Videos) – 20 Minuten, gleichzeitige
    /// Anfragen für dasselbe Video teilen sich einen Abruf (Vorladen + Klick)
    details: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, DetailSlot)>>>,
}

type DetailSlot = std::sync::Arc<tokio::sync::OnceCell<std::result::Result<VideoDetails, String>>>;
const DETAILS_TTL: Duration = Duration::from_secs(20 * 60);

/// Sprache, Region, Jugendschutz – pro Anfrage aus den Einstellungen des Kontos.
#[derive(Clone, Debug)]
pub struct Locale {
    pub hl: String,
    pub gl: String,
    pub safe: bool,
}

impl Default for Locale {
    fn default() -> Self {
        Self { hl: "de".into(), gl: "CH".into(), safe: false }
    }
}

impl Locale {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            hl: if s.language.is_empty() { "de".into() } else { s.language.clone() },
            gl: if s.region.is_empty() { "CH".into() } else { s.region.to_uppercase() },
            safe: s.family_friendly,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchFilter {
    /// "relevance", "date", "views", "rating"
    pub sort: String,
    /// "hour", "today", "week", "month", "year"
    pub date: String,
    /// "video", "channel", "playlist", "movie"
    pub kind: String,
    /// "short", "medium", "long"
    pub duration: String,
    /// "hd", "subtitles", "live", "4k", "creative_commons"
    pub features: Vec<String>,
}

fn varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

impl SearchFilter {
    /// Protobuf `params` der Suche (wie in Invidious' `produce_search_params`).
    pub fn params(&self) -> Option<String> {
        let mut filters = Vec::new();
        let field = |f: u64, v: u64, out: &mut Vec<u8>| {
            varint(f << 3, out);
            varint(v, out);
        };
        match self.date.as_str() {
            "hour" => field(1, 1, &mut filters),
            "today" => field(1, 2, &mut filters),
            "week" => field(1, 3, &mut filters),
            "month" => field(1, 4, &mut filters),
            "year" => field(1, 5, &mut filters),
            _ => {}
        }
        match self.kind.as_str() {
            "video" => field(2, 1, &mut filters),
            "channel" => field(2, 2, &mut filters),
            "playlist" => field(2, 3, &mut filters),
            "movie" => field(2, 4, &mut filters),
            _ => {}
        }
        match self.duration.as_str() {
            "short" => field(3, 1, &mut filters),
            "long" => field(3, 2, &mut filters),
            "medium" => field(3, 3, &mut filters),
            _ => {}
        }
        for f in &self.features {
            match f.as_str() {
                "hd" => field(4, 1, &mut filters),
                "subtitles" => field(5, 1, &mut filters),
                "creative_commons" => field(6, 1, &mut filters),
                "live" => field(8, 1, &mut filters),
                "4k" => field(14, 1, &mut filters),
                _ => {}
            }
        }
        let mut out = Vec::new();
        match self.sort.as_str() {
            "rating" => field(1, 1, &mut out),
            "date" => field(1, 2, &mut out),
            "views" => field(1, 3, &mut out),
            _ => {}
        }
        if !filters.is_empty() {
            varint(2 << 3 | 2, &mut out);
            varint(filters.len() as u64, &mut out);
            out.extend(filters);
        }
        if out.is_empty() {
            return None;
        }
        Some(base64::engine::general_purpose::STANDARD.encode(out))
    }
}

/// Reiter eines Kanals und ihre `params` (Protobuf, base64)
fn tab_params(tab: &str) -> &'static str {
    match tab {
        "shorts" => "EgZzaG9ydHPyBgUKA5oBAA%3D%3D",
        "streams" => "EgdzdHJlYW1z8gYECgJ6AA%3D%3D",
        "playlists" => "EglwbGF5bGlzdHPyBgQKAkIA",
        "posts" => "EgVwb3N0c_IGBAoCSgA%3D",
        "community" => "Egljb21tdW5pdHnyBgQKAkoA",
        _ => "EgZ2aWRlb3PyBgQKAjoA",
    }
}

/// Themen-Kanäle von YouTube für die Trends-Seite
fn trending_channel(cat: &str) -> Option<&'static str> {
    Some(match cat {
        "music" => "UC-9-kyTW8ZkZNDHQJ6FgpwQ",
        "gaming" => "UCOpNcN46UbXVtpKMrmU4Abg",
        "live" => "UC4R8DWoMoI7CAwX8_LjQHig",
        "sports" => "UCEgdi0XIXXZ-qJOFPf4JSKw",
        "learning" => "UCtFRv9O2AHqOZjjynzrv-xg",
        "fashion" => "UCrpQ4p1Ql_hG8rKXIKM1MOQ",
        _ => return None,
    })
}

impl Yt {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(UA)
            .timeout(Duration::from_secs(20))
            .gzip(true)
            .brotli(true)
            .build()
            .expect("http client");
        Self { http, details: Default::default() }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    async fn call(&self, endpoint: &str, loc: &Locale, mut body: Value) -> Result<Value> {
        let ctx = json!({
            "client": {
                "clientName": "WEB",
                "clientVersion": CLIENT_VERSION,
                "hl": loc.hl,
                "gl": loc.gl,
                "userAgent": UA,
                "utcOffsetMinutes": 0,
            },
            "user": { "lockedSafetyMode": false, "enableSafetyMode": loc.safe },
            "request": { "useSsl": true },
        });
        body["context"] = ctx;
        let res = self
            .http
            .post(format!("{BASE}/{endpoint}?prettyPrint=false"))
            .header("X-YouTube-Client-Name", "1")
            .header("X-YouTube-Client-Version", CLIENT_VERSION)
            .header("Origin", "https://www.youtube.com")
            .header("Accept-Language", format!("{},en;q=0.8", loc.hl))
            .json(&body)
            .send()
            .await?;
        let status = res.status();
        let v: Value = res.json().await?;
        if !status.is_success() {
            let msg = s(&v, "error.message").unwrap_or_else(|| status.to_string());
            return Err(anyhow!("YouTube: {msg}"));
        }
        Ok(v)
    }

    // ------------------------------------------------------------ Suche

    pub async fn search(&self, q: &str, filter: &SearchFilter, cont: Option<&str>, loc: &Locale) -> Result<Page<Item>> {
        let body = match cont {
            Some(c) => json!({ "continuation": c }),
            None => {
                let mut b = json!({ "query": q });
                if let Some(p) = filter.params() {
                    b["params"] = json!(p);
                }
                b
            }
        };
        let v = self.call("search", loc, body).await?;
        let mut c = Collector::new();
        c.walk(&v);
        Ok(Page { items: c.items, continuation: c.continuation })
    }

    pub async fn suggest(&self, q: &str, loc: &Locale) -> Result<Vec<String>> {
        let url = format!(
            "https://suggestqueries.google.com/complete/search?client=firefox&ds=yt&oe=utf-8&hl={}&gl={}&q={}",
            loc.hl,
            loc.gl.to_lowercase(),
            urlencoding::encode(q)
        );
        let txt = self.http.get(url).send().await?.text().await?;
        let v: Value = serde_json::from_str(&txt)?;
        Ok(v.get(1)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).take(10).collect())
            .unwrap_or_default())
    }

    // ------------------------------------------------------------ Trends

    pub async fn trending(&self, cat: &str, loc: &Locale) -> Result<Vec<VideoItem>> {
        if let Some(ch) = trending_channel(cat) {
            let v = self.call("browse", loc, json!({ "browseId": ch })).await?;
            let mut c = Collector::new();
            c.walk(at(&v, "contents").unwrap_or(&v));
            let first_playlist = c.items.iter().find_map(|i| match i {
                Item::Playlist(p) => Some(p.id.clone()),
                _ => None,
            });
            let videos = c.videos();
            if videos.is_empty() {
                // Musik: nur Charts-Playlists -> die erste davon öffnen
                if let Some(pl) = first_playlist {
                    return Ok(self.playlist(&pl, None, loc).await?.videos);
                }
            }
            return Ok(videos);
        }
        // "Angesagt": meistgesehene Videos von heute in der Region (YouTube hat die alte
        // Trends-Seite abgeschafft, FEtrending antwortet nur noch mit einem Fehler).
        let filter = SearchFilter { sort: "views".into(), date: "today".into(), kind: "video".into(), ..Default::default() };
        let mut out = Vec::new();
        let terms = match (cat, loc.hl == "de") {
            ("news", true) => ["Nachrichten", "News"],
            ("news", false) => ["news", "breaking news"],
            (_, true) => ["", "neu"],
            _ => ["", "new"],
        };
        for term in terms {
            let q = if term.is_empty() { "*".to_string() } else { term.to_string() };
            if let Ok(p) = self.search(&q, &filter, None, loc).await {
                for it in p.items {
                    if let Item::Video(v) = it {
                        if !out.iter().any(|x: &VideoItem| x.id == v.id) && !v.short {
                            out.push(v);
                        }
                    }
                }
            }
            if out.len() >= 30 {
                break;
            }
        }
        out.sort_by_key(|v| std::cmp::Reverse(v.views.unwrap_or(0)));
        Ok(out)
    }

    // ------------------------------------------------------------ Video

    /// Video-Details mit Zwischenspeicher (ohne Playlist-Bezug)
    pub async fn video(&self, id: &str, list: Option<&str>, loc: &Locale) -> Result<VideoDetails> {
        if list.is_some() {
            return self.video_fresh(id, list, loc).await;
        }
        let key = format!("{id}:{}:{}:{}", loc.hl, loc.gl, loc.safe);
        let slot = {
            let mut c = self.details.lock().await;
            c.retain(|_, (t, s)| t.elapsed() < DETAILS_TTL || s.get().is_none());
            if let Some((_, s)) = c.get(&key) {
                if matches!(s.get(), Some(Err(_))) {
                    c.remove(&key);
                }
            }
            c.entry(key).or_insert_with(|| (std::time::Instant::now(), Default::default())).1.clone()
        };
        slot.get_or_init(|| async { self.video_fresh(id, None, loc).await.map_err(|e| e.to_string()) })
            .await
            .clone()
            .map_err(|e| anyhow!(e))
    }

    /// Im Hintergrund vorladen (für den Vorrat an ähnlichen Videos)
    pub fn warm(&self, id: &str, loc: &Locale) {
        let me = self.clone();
        let (id, loc) = (id.to_string(), loc.clone());
        tokio::spawn(async move {
            let _ = me.video(&id, None, &loc).await;
        });
    }

    async fn video_fresh(&self, id: &str, list: Option<&str>, loc: &Locale) -> Result<VideoDetails> {
        let mut body = json!({ "videoId": id, "racyCheckOk": true, "contentCheckOk": true });
        if let Some(l) = list {
            body["playlistId"] = json!(l);
        }
        let next_f = self.call("next", loc, body);
        let player_f = self.call(
            "player",
            loc,
            json!({ "videoId": id, "racyCheckOk": true, "contentCheckOk": true }),
        );
        let (next, player) = tokio::join!(next_f, player_f);
        let next = next?;
        let player = player.unwrap_or(Value::Null);
        Ok(parse_video(id, &next, &player))
    }

    pub async fn comments(&self, token: &str, loc: &Locale) -> Result<CommentPage> {
        let v = self.call("next", loc, json!({ "continuation": token })).await?;
        Ok(parse_comments(&v))
    }

    // ------------------------------------------------------------ Kanal

    pub async fn resolve(&self, url: &str, loc: &Locale) -> Result<Value> {
        let v = self.call("navigation/resolve_url", loc, json!({ "url": url })).await?;
        Ok(v.get("endpoint").cloned().unwrap_or(Value::Null))
    }

    /// `@name`, Kanal-URL oder UC-ID -> UC-ID
    pub async fn channel_id(&self, input: &str, loc: &Locale) -> Result<String> {
        let input = input.trim();
        if input.starts_with("UC") && input.len() == 24 {
            return Ok(input.to_string());
        }
        let url = if input.starts_with("http") {
            input.to_string()
        } else if input.starts_with('@') {
            format!("https://www.youtube.com/{input}")
        } else {
            format!("https://www.youtube.com/@{input}")
        };
        let ep = self.resolve(&url, loc).await?;
        s(&ep, "browseEndpoint.browseId").ok_or_else(|| anyhow!("Kanal nicht gefunden"))
    }

    /// Links aus dem „Über“-Fenster eines Kanals (Titel, Adresse) – YouTube lädt sie nach
    pub async fn channel_about_links(&self, id: &str, loc: &Locale) -> Result<Vec<(String, String)>> {
        fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
            match v {
                Value::Object(m) => {
                    for (k, x) in m {
                        if k == key { out.push(x); }
                        walk(x, key, out);
                    }
                }
                Value::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
                _ => {}
            }
        }
        let v = self.call("browse", loc, json!({ "browseId": id })).await?;
        let mut cmds = vec![];
        walk(&v, "continuationCommand", &mut cmds);
        let mut out: Vec<(String, String)> = vec![];
        for c in cmds.iter().take(1) {
            let Some(tok) = c.get("token").and_then(Value::as_str) else { continue };
            let Ok(about) = self.call("browse", loc, json!({ "continuation": tok })).await else { continue };
            let mut links = vec![];
            walk(&about, "channelExternalLinkViewModel", &mut links);
            for l in links {
                let title = l.pointer("/title/content").and_then(Value::as_str).unwrap_or("").to_string();
                let url = l.pointer("/link/content").and_then(Value::as_str).unwrap_or("");
                if !url.is_empty() {
                    let u = if url.starts_with("http") { url.to_string() } else { format!("https://{url}") };
                    out.push((title, u));
                }
            }
        }
        Ok(out)
    }

    pub async fn channel(&self, id: &str, loc: &Locale) -> Result<Channel> {
        let v = self.call("browse", loc, json!({ "browseId": id })).await?;
        if let Some(alert) = find(&v, "alertRenderer") {
            if v.get("contents").is_none() {
                return Err(anyhow!("{}", t(alert, "text")));
            }
        }
        Ok(parse_channel(id, &v))
    }

    pub async fn channel_tab(&self, id: &str, tab: &str, cont: Option<&str>, loc: &Locale) -> Result<(Page<Item>, Vec<Post>)> {
        let mut try_tabs = vec![tab];
        if tab == "posts" {
            try_tabs.push("community");
        }
        let mut last = Err(anyhow!("leer"));
        for t in try_tabs {
            let body = match cont {
                Some(c) => json!({ "continuation": c }),
                None => json!({ "browseId": id, "params": urlencoding::decode(tab_params(t)).map(|x| x.into_owned()).unwrap_or_default() }),
            };
            match self.call("browse", loc, body).await {
                Ok(v) => {
                    let mut c = Collector::new();
                    // Nur den gewählten Reiter (nicht den Kopf der Seite)
                    let root = selected_tab(&v).unwrap_or(&v);
                    c.walk(root);
                    if cont.is_none() {
                        if let Some(x) = v.get("onResponseReceivedActions") {
                            c.walk(x);
                        }
                    }
                    let (items, posts, contn) = (c.items, c.posts, c.continuation);
                    let mut items = items;
                    for it in items.iter_mut() {
                        if let Item::Video(vid) = it {
                            if tab == "shorts" {
                                vid.short = true;
                            }
                        }
                    }
                    if items.is_empty() && posts.is_empty() && cont.is_none() {
                        last = Ok((Page { items, continuation: contn }, posts));
                        continue;
                    }
                    return Ok((Page { items, continuation: contn }, posts));
                }
                Err(e) => last = Err(e),
            }
        }
        last
    }

    pub async fn channel_search(&self, id: &str, q: &str, loc: &Locale) -> Result<Page<Item>> {
        let v = self
            .call("browse", loc, json!({ "browseId": id, "params": "EgZzZWFyY2jyBgQKAloA", "query": q }))
            .await?;
        let mut c = Collector::new();
        c.walk(selected_tab(&v).unwrap_or(&v));
        Ok(Page { items: c.items, continuation: c.continuation })
    }

    // ------------------------------------------------------------ Playlist

    pub async fn playlist(&self, id: &str, cont: Option<&str>, loc: &Locale) -> Result<PlaylistDetails> {
        let body = match cont {
            Some(c) => json!({ "continuation": c }),
            None => json!({ "browseId": if id.starts_with("VL") { id.to_string() } else { format!("VL{id}") } }),
        };
        let v = self.call("browse", loc, body).await?;
        let mut c = Collector::new();
        c.walk(at(&v, "contents").or_else(|| v.get("onResponseReceivedActions")).unwrap_or(&v));
        let mut d = PlaylistDetails { id: id.to_string(), ..Default::default() };
        if cont.is_none() {
            if let Some(m) = at(&v, "metadata.playlistMetadataRenderer") {
                d.title = t(m, "title");
                d.description = t(m, "description");
            }
            if let Some(h) = find(&v, "playlistHeaderRenderer") {
                if d.title.is_empty() {
                    d.title = t(h, "title");
                }
                d.author = t(h, "ownerText");
                d.author_id = h.get("ownerText").and_then(channel_from_endpoint).unwrap_or_default();
                d.video_count = opt_t(h, "numVideosText");
                d.views = opt_t(h, "viewCountText");
            }
            if let Some(h) = find(&v, "pageHeaderViewModel") {
                if d.title.is_empty() {
                    d.title = t(h, "title.dynamicTextViewModel.text");
                }
                let mut parts = Vec::new();
                find_all(h, "metadataParts", &mut parts);
                for p in parts.iter().filter_map(|p| p.as_array()).flatten() {
                    let txt = t(p, "text");
                    if d.author.is_empty() && txt.starts_with("von ") || txt.starts_with("by ") {
                        d.author = txt.trim_start_matches("von ").trim_start_matches("by ").to_string();
                    }
                    if let Some(chid) = s(p, "text.commandRuns.0.onTap.innertubeCommand.browseEndpoint.browseId") {
                        d.author_id = chid;
                        if d.author.is_empty() {
                            d.author = txt.clone();
                        }
                    } else if txt.contains("Video") || txt.contains("video") {
                        d.video_count = Some(txt.clone());
                    } else if txt.contains("Aufruf") || txt.contains("view") {
                        d.views = Some(txt.clone());
                    }
                }
                if d.description.is_empty() {
                    d.description = t(h, "description.descriptionPreviewViewModel.description");
                }
            }
            if d.title.is_empty() {
                if let Some(alert) = find(&v, "alertRenderer") {
                    return Err(anyhow!("{}", t(alert, "text")));
                }
            }
        }
        d.continuation = c.continuation.clone();
        d.videos = c.videos();
        d.thumb = d.videos.first().map(|x| x.thumb.clone()).unwrap_or_default();
        Ok(d)
    }
}

fn selected_tab(v: &Value) -> Option<&Value> {
    let tabs = at(v, "contents.twoColumnBrowseResultsRenderer.tabs")?.as_array()?;
    tabs.iter()
        .find(|t| at(t, "tabRenderer.selected").and_then(Value::as_bool).unwrap_or(false))
        .or_else(|| tabs.iter().find(|t| at(t, "expandableTabRenderer.selected").and_then(Value::as_bool).unwrap_or(false)))
        .and_then(|t| t.get("tabRenderer").or_else(|| t.get("expandableTabRenderer")))
        .and_then(|t| t.get("content"))
}

fn parse_channel(id: &str, v: &Value) -> Channel {
    let mut ch = Channel { id: id.to_string(), ..Default::default() };
    if let Some(m) = at(v, "metadata.channelMetadataRenderer") {
        ch.name = t(m, "title");
        ch.description = t(m, "description");
        ch.avatar = best_thumb(m.get("avatar")).map(|u| crate::proxy::img(&u)).unwrap_or_default();
        if let Some(ext) = s(m, "externalId") {
            ch.id = ext;
        }
        ch.family_safe = m.get("isFamilySafe").and_then(Value::as_bool).unwrap_or(true);
        if let Some(van) = s(m, "vanityChannelUrl") {
            if let Some(h) = van.rsplit('/').next() {
                if h.starts_with('@') {
                    ch.handle = Some(h.to_string());
                }
            }
        }
    }
    if let Some(h) = find(v, "pageHeaderViewModel") {
        if ch.name.is_empty() {
            ch.name = t(h, "title.dynamicTextViewModel.text");
        }
        if let Some(b) = best_thumb(at(h, "banner.imageBannerViewModel.image")) {
            ch.banner = Some(crate::proxy::img(&b));
        }
        if ch.avatar.is_empty() {
            if let Some(a) = best_thumb(at(h, "image.decoratedAvatarViewModel.avatar.avatarViewModel.image")) {
                ch.avatar = crate::proxy::img(&a);
            }
        }
        let mut parts = Vec::new();
        find_all(h, "metadataParts", &mut parts);
        for p in parts.iter().filter_map(|p| p.as_array()).flatten() {
            let txt = t(p, "text");
            if txt.starts_with('@') {
                ch.handle = Some(txt);
            } else if txt.contains("Abonnent") || txt.contains("subscriber") {
                ch.subscribers = Some(txt);
            } else if txt.contains("Video") || txt.contains("video") {
                ch.video_count = Some(txt);
            }
        }
        let mut badges = Vec::new();
        find_all(h, "icon", &mut badges);
        ch.verified = find(h, "CHECK_CIRCLE_FILLED").is_some()
            || serde_json::to_string(h).map(|x| x.contains("CHECK_CIRCLE")).unwrap_or(false);
    }
    if let Some(h) = find(v, "c4TabbedHeaderRenderer") {
        if ch.subscribers.is_none() {
            ch.subscribers = opt_t(h, "subscriberCountText");
        }
        if ch.banner.is_none() {
            ch.banner = best_thumb(h.get("banner")).map(|u| crate::proxy::img(&u));
        }
    }
    if let Some(Value::Array(tabs)) = at(v, "contents.twoColumnBrowseResultsRenderer.tabs") {
        for tab in tabs {
            let url = s(tab, "tabRenderer.endpoint.commandMetadata.webCommandMetadata.url").unwrap_or_default();
            let name = url.rsplit('/').next().unwrap_or_default();
            let key = match name {
                "videos" => "videos",
                "shorts" => "shorts",
                "streams" => "streams",
                "playlists" => "playlists",
                "posts" | "community" => "posts",
                _ => continue,
            };
            ch.tabs.push(key.to_string());
        }
    }
    if ch.tabs.is_empty() {
        ch.tabs = vec!["videos".into()];
    }
    ch
}

fn parse_video(id: &str, next: &Value, player: &Value) -> VideoDetails {
    let mut d = VideoDetails { id: id.to_string(), thumb: crate::proxy::video_thumb(id), family_safe: true, ..Default::default() };
    let results = at(next, "contents.twoColumnWatchNextResults.results.results.contents").unwrap_or(&Value::Null);
    if let Some(p) = find(results, "videoPrimaryInfoRenderer") {
        d.title = t(p, "title");
        d.views_text = opt_t(p, "viewCount.videoViewCountRenderer.viewCount");
        d.views = s(p, "viewCount.videoViewCountRenderer.originalViewCount")
            .and_then(|x| x.parse().ok())
            .filter(|x: &u64| *x > 0)
            .or_else(|| d.views_text.as_deref().and_then(parse_count));
        d.date_text = opt_t(p, "dateText");
        d.relative_date = opt_t(p, "relativeDateText");
        d.live = at(p, "viewCount.videoViewCountRenderer.isLive").and_then(Value::as_bool).unwrap_or(false);
        if let Some(like) = find(p, "likeButtonViewModel") {
            if let Some(bvm) = find(like, "buttonViewModel") {
                d.likes_text = s(bvm, "title").filter(|x| !x.is_empty() && x.chars().any(|c| c.is_ascii_digit()));
            }
        }
    }
    if let Some(sec) = find(results, "videoSecondaryInfoRenderer") {
        if let Some(o) = at(sec, "owner.videoOwnerRenderer") {
            d.author = t(o, "title");
            d.author_id = channel_from_endpoint(o).unwrap_or_default();
            d.author_thumb = best_thumb(o.get("thumbnail")).map(|u| crate::proxy::img(&u));
            d.subscribers = opt_t(o, "subscriberCountText");
            d.verified = serde_json::to_string(o.get("badges").unwrap_or(&Value::Null)).map(|x| x.contains("VERIFIED")).unwrap_or(false);
        }
        d.description = opt_t(sec, "attributedDescription").unwrap_or_else(|| t(sec, "description"));
    }
    // Kommentare: Startpunkt
    let mut sections = Vec::new();
    find_all(results, "itemSectionRenderer", &mut sections);
    for sec in sections {
        if s(sec, "sectionIdentifier").as_deref() == Some("comment-item-section") {
            d.comments_token = find(sec, "continuationCommand").and_then(|c| s(c, "token"));
        }
    }
    if let Some(h) = find(results, "commentsEntryPointHeaderRenderer") {
        d.comment_count = opt_t(h, "commentCount");
    }
    if let Some(Value::Array(panels)) = next.get("engagementPanels") {
        for p in panels {
            let ident = s(p, "engagementPanelSectionListRenderer.panelIdentifier").unwrap_or_default();
            if ident == "engagement-panel-comments-section" && d.comment_count.is_none() {
                d.comment_count = opt_t(p, "engagementPanelSectionListRenderer.header.engagementPanelTitleHeaderRenderer.contextualInfo");
            }
        }
        // Kapitel
        let mut markers = Vec::new();
        for p in panels {
            find_all(p, "macroMarkersListItemRenderer", &mut markers);
        }
        for m in markers {
            let start = at(m, "onTap.watchEndpoint.startTimeSeconds").and_then(Value::as_f64).unwrap_or(0.0);
            let title = t(m, "title");
            if !title.is_empty() && !d.chapters.iter().any(|c| (c.start - start).abs() < 0.5) {
                d.chapters.push(Chapter {
                    title,
                    start,
                    thumb: best_thumb(m.get("thumbnail")).map(|u| crate::proxy::img(&u)),
                });
            }
        }
        d.chapters.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    if d.chapters.is_empty() {
        d.chapters = chapters_from_description(&d.description);
    }
    // Ähnliche Videos
    if let Some(sec) = at(next, "contents.twoColumnWatchNextResults.secondaryResults") {
        let mut c = Collector::new();
        c.walk(sec);
        d.related = c.videos().into_iter().filter(|v| v.id != id).collect();
    }
    // Playlist
    if let Some(pl) = at(next, "contents.twoColumnWatchNextResults.playlist.playlist") {
        let mut c = Collector::new();
        c.walk(pl.get("contents").unwrap_or(&Value::Null));
        let videos = c.videos();
        let index = at(pl, "currentIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
        d.playlist = Some(WatchPlaylist {
            id: s(pl, "playlistId").unwrap_or_default(),
            title: t(pl, "title"),
            author: t(pl, "ownerName"),
            index: videos.iter().position(|v| v.id == id).unwrap_or(index),
            videos,
        });
    }
    // Player: Dauer, Jugendschutz, Schlagwörter
    if let Some(vd) = player.get("videoDetails") {
        d.duration = s(vd, "lengthSeconds").and_then(|x| x.parse().ok()).filter(|x| *x > 0);
        if d.title.is_empty() {
            d.title = s(vd, "title").unwrap_or_default();
        }
        if d.author.is_empty() {
            d.author = s(vd, "author").unwrap_or_default();
            d.author_id = s(vd, "channelId").unwrap_or_default();
        }
        if d.description.is_empty() {
            d.description = s(vd, "shortDescription").unwrap_or_default();
        }
        if d.views.is_none() {
            d.views = s(vd, "viewCount").and_then(|x| x.parse().ok());
        }
        if let Some(Value::Array(k)) = vd.get("keywords") {
            d.keywords = k.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
        }
        if vd.get("isLive").and_then(Value::as_bool).unwrap_or(false) {
            d.live = true;
        }
    }
    if let Some(mf) = at(player, "microformat.playerMicroformatRenderer") {
        d.family_safe = mf.get("isFamilySafe").and_then(Value::as_bool).unwrap_or(true);
        d.category = s(mf, "category");
        if d.date_text.is_none() {
            d.date_text = s(mf, "publishDate");
        }
    }
    d
}

/// Kapitel aus Zeitmarken in der Beschreibung ("0:00 Intro")
pub fn chapters_from_description(desc: &str) -> Vec<Chapter> {
    let mut out = Vec::new();
    for line in desc.lines() {
        let line = line.trim();
        let mut words = line.splitn(2, char::is_whitespace);
        let first = words.next().unwrap_or("");
        let ts = first.trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']');
        if ts.contains(':') {
            if let Some(sec) = parse_duration(ts) {
                let title = words.next().unwrap_or("").trim_start_matches(['-', '–', ':', '|', ' ']).trim().to_string();
                if !title.is_empty() {
                    out.push(Chapter { title, start: sec as f64, thumb: None });
                }
            }
        }
    }
    // Nur gültig, wenn bei 0:00 beginnend und mindestens drei Kapitel (YouTube-Regel)
    if out.len() >= 3 && out[0].start == 0.0 && out.windows(2).all(|w| w[0].start < w[1].start) {
        out
    } else {
        Vec::new()
    }
}

fn parse_comments(v: &Value) -> CommentPage {
    let mut page = CommentPage::default();
    // Neues Format: Daten in frameworkUpdates, Verweise über commentKey
    let mut entities = std::collections::HashMap::new();
    if let Some(Value::Array(muts)) = at(v, "frameworkUpdates.entityBatchUpdate.mutations") {
        for m in muts {
            if let (Some(key), Some(p)) = (s(m, "entityKey"), at(m, "payload")) {
                entities.insert(key, p.clone());
            }
        }
    }
    let mut items: Vec<&Value> = Vec::new();
    for key in ["onResponseReceivedEndpoints"] {
        if let Some(Value::Array(eps)) = v.get(key) {
            for ep in eps {
                let list = at(ep, "reloadContinuationItemsCommand.continuationItems")
                    .or_else(|| at(ep, "appendContinuationItemsAction.continuationItems"));
                if let Some(Value::Array(l)) = list {
                    items.extend(l.iter());
                }
                // Kopf: Anzahl und Sortierung
                if let Some(h) = find(ep, "commentsHeaderRenderer") {
                    page.count = opt_t(h, "countText").or_else(|| opt_t(h, "commentsCount"));
                    if let Some(Value::Array(sub)) = at(h, "sortMenu.sortFilterSubMenuRenderer.subMenuItems") {
                        let toks: Vec<Option<String>> = sub
                            .iter()
                            .map(|x| find(x, "continuationCommand").and_then(|c| s(c, "token")))
                            .collect();
                        page.top_token = toks.first().cloned().flatten();
                        page.newest_token = toks.get(1).cloned().flatten();
                    }
                }
            }
        }
    }
    for it in items {
        if let Some(th) = it.get("commentThreadRenderer") {
            let replies_token = at(th, "replies.commentRepliesRenderer")
                .and_then(|r| find(r, "continuationCommand"))
                .and_then(|c| s(c, "token"));
            let c = if let Some(vm) = at(th, "commentViewModel.commentViewModel").or_else(|| th.get("commentViewModel")) {
                comment_vm(vm, &entities)
            } else {
                th.get("comment").and_then(|c| c.get("commentRenderer")).map(comment_renderer)
            };
            if let Some(mut c) = c {
                c.replies_token = replies_token;
                page.comments.push(c);
            }
        } else if let Some(vm) = it.get("commentViewModel") {
            if let Some(c) = comment_vm(vm, &entities) {
                page.comments.push(c);
            }
        } else if let Some(cr) = it.get("commentRenderer") {
            page.comments.push(comment_renderer(cr));
        } else if let Some(ci) = it.get("continuationItemRenderer") {
            page.continuation = find(ci, "continuationCommand").and_then(|c| s(c, "token"));
        }
    }
    page
}

fn comment_vm(vm: &Value, entities: &std::collections::HashMap<String, Value>) -> Option<Comment> {
    let key = s(vm, "commentKey")?;
    let p = entities.get(&key)?.get("commentEntityPayload")?;
    let toolbar_key = s(vm, "toolbarStateKey").unwrap_or_default();
    let hearted = entities
        .get(&toolbar_key)
        .and_then(|e| s(e, "engagementToolbarStateEntityPayload.heartState"))
        .is_some_and(|h| h.contains("HEARTED") && !h.contains("UNHEARTED"));
    let reply_count = s(p, "toolbar.replyCount")
        .and_then(|x| crate::yt::json::parse_count_approx(&x))
        .unwrap_or(0) as u32;
    Some(Comment {
        id: s(p, "properties.commentId").unwrap_or_default(),
        author: s(p, "author.displayName").unwrap_or_default(),
        author_id: s(p, "author.channelId").unwrap_or_default(),
        author_thumb: s(p, "author.avatarThumbnailUrl").map(|u| crate::proxy::img(&u)).unwrap_or_default(),
        text: s(p, "properties.content.content").unwrap_or_default(),
        likes: s(p, "toolbar.likeCountNotliked").unwrap_or_default().trim().to_string(),
        published: s(p, "properties.publishedTime").unwrap_or_default(),
        reply_count,
        replies_token: None,
        pinned: vm.get("pinnedText").is_some(),
        hearted,
        by_owner: at(p, "author.isCreator").and_then(Value::as_bool).unwrap_or(false),
        verified: at(p, "author.isVerified").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn comment_renderer(c: &Value) -> Comment {
    Comment {
        id: s(c, "commentId").unwrap_or_default(),
        author: t(c, "authorText"),
        author_id: s(c, "authorEndpoint.browseEndpoint.browseId").unwrap_or_default(),
        author_thumb: best_thumb(c.get("authorThumbnail")).map(|u| crate::proxy::img(&u)).unwrap_or_default(),
        text: t(c, "contentText"),
        likes: t(c, "voteCount"),
        published: t(c, "publishedTimeText"),
        reply_count: c.get("replyCount").and_then(Value::as_u64).unwrap_or(0) as u32,
        replies_token: None,
        pinned: c.get("pinnedCommentBadge").is_some(),
        hearted: c.get("actionButtons").map(|a| find(a, "creatorHeart").is_some()).unwrap_or(false),
        by_owner: c.get("authorIsChannelOwner").and_then(Value::as_bool).unwrap_or(false),
        verified: false,
    }
}
