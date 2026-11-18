//! Export und Import: Abos (FreeTube, NewPipe, YouTube-Takeout-CSV, OPML, Invidious) sowie
//! Playlists und Verlauf im FreeTube-Format.

use crate::db::{now, Db};
use nrztube_types::*;
use serde_json::{json, Value};

fn channel_from_url(u: &str) -> Option<String> {
    let i = u.find("channel/").map(|i| i + 8).or_else(|| u.find("channel_id=").map(|i| i + 11))?;
    let id: String = u[i..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    (id.starts_with("UC") && id.len() == 24).then_some(id)
}

fn xml_attr(el: &str, a: &str) -> Option<String> {
    let key = format!("{a}=\"");
    let i = el.find(&key)? + key.len();
    el[i..].split('"').next().map(|x| {
        x.replace("&amp;", "&").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&#39;", "'")
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

pub struct Parsed {
    pub subs: Vec<Subscription>,
    /// FreeTube-Profile: (Name, Farbe, Kanäle)
    pub profiles: Vec<(String, String, Vec<String>)>,
    pub format: String,
}

/// Erkennt das Format und liest die Abos.
pub fn parse_subscriptions(text: &str) -> Parsed {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    let mut p = Parsed { subs: vec![], profiles: vec![], format: String::new() };
    let add = |subs: &mut Vec<Subscription>, id: String, name: String, thumb: String| {
        if !subs.iter().any(|s| s.channel_id == id) {
            subs.push(Subscription { channel_id: id, name, thumb, added: now() });
        }
    };
    // OPML
    if trimmed.starts_with('<') {
        p.format = "OPML".into();
        for el in trimmed.split("<outline").skip(1) {
            let el = el.split('>').next().unwrap_or("");
            let url = xml_attr(el, "xmlUrl").unwrap_or_default();
            if let Some(id) = channel_from_url(&url) {
                let name = xml_attr(el, "title").or_else(|| xml_attr(el, "text")).unwrap_or_default();
                add(&mut p.subs, id, name, String::new());
            }
        }
        return p;
    }
    // JSON (NewPipe, Invidious) oder JSON-Zeilen (FreeTube)
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            if let Some(Value::Array(list)) = v.get("subscriptions") {
                if v.get("app_version").is_some() || list.first().is_some_and(|x| x.get("service_id").is_some()) {
                    p.format = "NewPipe".into();
                    for s in list {
                        let url = s.get("url").and_then(Value::as_str).unwrap_or("");
                        if let Some(id) = channel_from_url(url) {
                            add(&mut p.subs, id, s.get("name").and_then(Value::as_str).unwrap_or("").into(), String::new());
                        }
                    }
                } else {
                    p.format = "Invidious".into();
                    for s in list {
                        if let Some(id) = s.as_str() {
                            add(&mut p.subs, id.to_string(), String::new(), String::new());
                        }
                    }
                }
                return p;
            }
            // Einzelnes FreeTube-Profil
            if v.get("subscriptions").is_none() {
                if let Value::Array(list) = &v {
                    p.format = "JSON".into();
                    for s in list {
                        if let Some(id) = s.get("id").or_else(|| s.get("channel_id")).and_then(Value::as_str) {
                            add(&mut p.subs, id.to_string(), s.get("name").and_then(Value::as_str).unwrap_or("").into(), String::new());
                        }
                    }
                    return p;
                }
            }
        }
        // FreeTube: eine Zeile pro Profil
        p.format = "FreeTube".into();
        for line in trimmed.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let name = v.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let color = v.get("bgColor").and_then(Value::as_str).unwrap_or("").to_string();
            let is_all = v.get("_id").and_then(Value::as_str) == Some("allChannels");
            let mut chans = Vec::new();
            if let Some(Value::Array(list)) = v.get("subscriptions") {
                for s in list {
                    let Some(id) = s.get("id").and_then(Value::as_str) else { continue };
                    let thumb = s.get("thumbnail").and_then(Value::as_str).map(crate::proxy::img).unwrap_or_default();
                    add(&mut p.subs, id.to_string(), s.get("name").and_then(Value::as_str).unwrap_or("").into(), thumb);
                    chans.push(id.to_string());
                }
            }
            if !is_all && !name.is_empty() {
                p.profiles.push((name, color, chans));
            }
        }
        return p;
    }
    // CSV aus Google Takeout: "Channel Id,Channel Url,Channel Title" (Spaltennamen je nach Sprache)
    p.format = "CSV".into();
    for line in trimmed.lines() {
        let cols: Vec<&str> = line.split(',').collect();
        let Some(first) = cols.first() else { continue };
        let id = first.trim().trim_matches('"');
        let id = if id.starts_with("UC") && id.len() == 24 {
            id.to_string()
        } else if let Some(id) = cols.get(1).and_then(|u| channel_from_url(u)) {
            id
        } else {
            continue;
        };
        let name = cols.get(2..).map(|c| c.join(",")).unwrap_or_default().trim().trim_matches('"').to_string();
        add(&mut p.subs, id, name, String::new());
    }
    p
}

pub fn export_subscriptions(db: &Db, user: i64, format: &str) -> (String, &'static str, String) {
    let subs = db.subscriptions(user);
    let date = chrono::Utc::now().format("%Y-%m-%d");
    match format {
        "newpipe" => {
            let list: Vec<Value> = subs
                .iter()
                .map(|s| json!({ "service_id": 0, "url": format!("https://www.youtube.com/channel/{}", s.channel_id), "name": s.name }))
                .collect();
            let body = json!({ "app_version": "0.27.0", "app_version_int": 1000, "subscriptions": list });
            (serde_json::to_string_pretty(&body).unwrap_or_default(), "application/json", format!("nrztube-newpipe-{date}.json"))
        }
        "csv" => {
            let mut out = String::from("Channel Id,Channel Url,Channel Title\n");
            for s in &subs {
                out += &format!("{},https://www.youtube.com/channel/{},{}\n", s.channel_id, s.channel_id, s.name.replace(',', " "));
            }
            (out, "text/csv", format!("nrztube-subscriptions-{date}.csv"))
        }
        "opml" => {
            let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"1.1\">\n<body>\n<outline text=\"YouTube Subscriptions\" title=\"YouTube Subscriptions\">\n");
            for s in &subs {
                out += &format!(
                    "<outline text=\"{n}\" title=\"{n}\" type=\"rss\" xmlUrl=\"https://www.youtube.com/feeds/videos.xml?channel_id={id}\"/>\n",
                    n = esc(&s.name),
                    id = s.channel_id
                );
            }
            out += "</outline>\n</body>\n</opml>\n";
            (out, "text/x-opml", format!("nrztube-subscriptions-{date}.opml"))
        }
        _ => {
            // FreeTube: "Alle Kanäle" + jedes Profil als eigene Zeile
            let ft = |s: &Subscription| json!({ "id": s.channel_id, "name": s.name, "thumbnail": "" });
            let mut out = serde_json::to_string(&json!({
                "_id": "allChannels", "name": "All Channels", "bgColor": "#000000", "textColor": "#FFFFFF",
                "subscriptions": subs.iter().map(ft).collect::<Vec<_>>()
            }))
            .unwrap_or_default();
            out.push('\n');
            for p in db.profiles(user) {
                let list: Vec<Value> = subs.iter().filter(|s| p.channels.contains(&s.channel_id)).map(ft).collect();
                out += &serde_json::to_string(&json!({
                    "_id": format!("nrztube-{}", p.id), "name": p.name,
                    "bgColor": if p.color.is_empty() { "#555555".to_string() } else { p.color.clone() },
                    "textColor": "#FFFFFF", "subscriptions": list
                }))
                .unwrap_or_default();
                out.push('\n');
            }
            (out, "application/x-ndjson", format!("nrztube-freetube-subscriptions-{date}.db"))
        }
    }
}

// ------------------------------------------------------------ Playlists und Verlauf (FreeTube)

fn ft_video(v: &VideoItem) -> Value {
    json!({
        "videoId": v.id, "title": v.title, "author": v.author, "authorId": v.author_id,
        "lengthSeconds": v.duration.unwrap_or(0), "type": "video",
    })
}

fn video_from_ft(v: &Value) -> Option<VideoItem> {
    let id = v.get("videoId").and_then(Value::as_str)?.to_string();
    Some(VideoItem {
        thumb: crate::proxy::video_thumb(&id),
        title: v.get("title").and_then(Value::as_str).unwrap_or("").into(),
        author: v.get("author").and_then(Value::as_str).unwrap_or("").into(),
        author_id: v.get("authorId").and_then(Value::as_str).unwrap_or("").into(),
        duration: v.get("lengthSeconds").and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).map(|x| x as u32).filter(|x| *x > 0),
        views: v.get("viewCount").and_then(Value::as_u64),
        live: v.get("isLive").and_then(Value::as_bool).unwrap_or(false),
        id,
        ..Default::default()
    })
}

pub fn export_playlists(db: &Db, user: i64) -> String {
    let mut out = String::new();
    for p in db.playlists(user) {
        let Some(full) = db.playlist(user, p.id) else { continue };
        let videos: Vec<Value> = full
            .videos
            .iter()
            .map(|v| {
                let mut x = ft_video(v);
                x["timeAdded"] = json!(now() * 1000);
                x
            })
            .collect();
        let name = match (p.protected, p.title.as_str()) {
            (true, "Favoriten") => "Favorites".to_string(),
            (true, "Später ansehen") => "Watch Later".to_string(),
            _ => p.title.clone(),
        };
        out += &serde_json::to_string(&json!({
            "playlistName": name, "protected": p.protected, "description": p.description,
            "videos": videos, "_id": format!("nrztube-{}", p.id), "createdAt": p.updated * 1000, "lastUpdatedAt": p.updated * 1000
        }))
        .unwrap_or_default();
        out.push('\n');
    }
    out
}

pub fn import_playlists(db: &Db, user: i64, text: &str) -> ImportResult {
    let mut r = ImportResult { format: "FreeTube".into(), ..Default::default() };
    let existing = db.playlists(user);
    let lines: Vec<Value> = if text.trim_start().starts_with('[') {
        serde_json::from_str::<Vec<Value>>(text).unwrap_or_default()
    } else {
        text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    };
    for v in lines {
        let name = v.get("playlistName").or_else(|| v.get("title")).and_then(Value::as_str).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let local = match name.as_str() {
            "Favorites" => "Favoriten".to_string(),
            "Watch Later" => "Später ansehen".to_string(),
            _ => name,
        };
        let id = match existing.iter().find(|p| p.title == local) {
            Some(p) => p.id,
            None => match db.create_playlist(user, &local, v.get("description").and_then(Value::as_str).unwrap_or("")) {
                Ok(id) => id,
                Err(_) => continue,
            },
        };
        if let Some(Value::Array(videos)) = v.get("videos") {
            for x in videos {
                let item = video_from_ft(x).or_else(|| {
                    // Invidious: nur IDs
                    x.as_str().map(|id| VideoItem { id: id.into(), thumb: crate::proxy::video_thumb(id), ..Default::default() })
                });
                match item.map(|it| db.add_to_playlist(user, id, &it)) {
                    Some(Ok(true)) => r.added += 1,
                    _ => r.skipped += 1,
                }
            }
        }
    }
    r
}

pub fn export_history(db: &Db, user: i64) -> String {
    let mut out = String::new();
    for h in db.history(user, "", 1_000_000, 0) {
        let mut x = ft_video(&h.video);
        x["watchProgress"] = json!(h.progress);
        x["timeWatched"] = json!(h.watched * 1000);
        x["viewCount"] = json!(h.video.views.unwrap_or(0));
        x["isLive"] = json!(h.video.live);
        x["_id"] = json!(h.video.id);
        out += &serde_json::to_string(&x).unwrap_or_default();
        out.push('\n');
    }
    out
}

pub fn import_history(db: &Db, user: i64, text: &str) -> ImportResult {
    let mut r = ImportResult { format: "FreeTube".into(), ..Default::default() };
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(item) = video_from_ft(&v) else {
            r.skipped += 1;
            continue;
        };
        let progress = v.get("watchProgress").and_then(Value::as_f64).unwrap_or(0.0);
        let watched = v.get("timeWatched").and_then(Value::as_i64).map(|t| t / 1000);
        if db.add_history(user, &item, progress, watched).is_ok() {
            r.added += 1;
        } else {
            r.skipped += 1;
        }
    }
    r
}
