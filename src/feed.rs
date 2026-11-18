//! Abo-Feed über die RSS-Feeds der Kanäle (schnell, ohne Konto – wie FreeTube und Invidious).

use futures::StreamExt;
use nrztube_types::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::proxy::video_thumb;

#[derive(Clone)]
struct Entry {
    fetched: Instant,
    videos: Arc<Vec<VideoItem>>,
}

pub struct Feeds {
    http: reqwest::Client,
    cache: Mutex<HashMap<String, Entry>>,
}

const FRESH: Duration = Duration::from_secs(15 * 60);

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}");
    let start = s.find(&open)?;
    let after = &s[start + open.len()..];
    let gt = after.find('>')?;
    if after[..gt].ends_with('/') {
        return Some("");
    }
    let body = &after[gt + 1..];
    let end = body.find(&format!("</{name}>"))?;
    Some(&body[..end])
}

fn attr<'a>(s: &'a str, name: &str, a: &str) -> Option<&'a str> {
    let open = format!("<{name} ");
    let start = s.find(&open)?;
    let after = &s[start..];
    let end = after.find('>')?;
    let el = &after[..end];
    let key = format!("{a}=\"");
    let i = el.find(&key)? + key.len();
    el[i..].split('"').next()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

pub fn parse_rss(xml: &str) -> (String, Vec<VideoItem>) {
    let channel_name = xml.split("<entry>").next().and_then(|h| tag(h, "title")).map(unescape).unwrap_or_default();
    let mut out = Vec::new();
    for entry in xml.split("<entry>").skip(1) {
        let Some(id) = tag(entry, "yt:videoId") else { continue };
        let published = tag(entry, "published")
            .and_then(|p| chrono::DateTime::parse_from_rfc3339(p).ok())
            .map(|d| d.timestamp());
        let link = attr(entry, "link", "href").unwrap_or("");
        let views = attr(entry, "media:statistics", "views").and_then(|v| v.parse::<u64>().ok());
        let author_id = tag(entry, "yt:channelId").unwrap_or("").to_string();
        let author = tag(entry, "author").and_then(|a| tag(a, "name")).map(unescape).unwrap_or_default();
        out.push(VideoItem {
            id: id.to_string(),
            title: tag(entry, "title").map(unescape).unwrap_or_default(),
            author,
            author_id,
            thumb: video_thumb(id),
            views,
            published_ts: published,
            short: link.contains("/shorts/"),
            description: tag(entry, "media:description").map(unescape).filter(|d| !d.is_empty()),
            ..Default::default()
        });
    }
    (channel_name, out)
}

impl Feeds {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, cache: Mutex::new(HashMap::new()) }
    }

    pub async fn channel(&self, id: &str, force: bool) -> Arc<Vec<VideoItem>> {
        if !force {
            if let Some(e) = self.cache.lock().await.get(id) {
                if e.fetched.elapsed() < FRESH {
                    return e.videos.clone();
                }
            }
        }
        let url = format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}");
        let res = self.http.get(&url).timeout(Duration::from_secs(15)).send().await;
        let videos = match res {
            Ok(r) if r.status().is_success() => r.text().await.map(|x| parse_rss(&x).1).unwrap_or_default(),
            _ => {
                // Bei Fehlern die alten Daten behalten
                if let Some(e) = self.cache.lock().await.get(id) {
                    return e.videos.clone();
                }
                Vec::new()
            }
        };
        let arc = Arc::new(videos);
        self.cache.lock().await.insert(id.to_string(), Entry { fetched: Instant::now(), videos: arc.clone() });
        arc
    }

    pub async fn many(&self, ids: &[String], force: bool) -> Vec<VideoItem> {
        let results: Vec<Arc<Vec<VideoItem>>> = futures::stream::iter(ids.iter().cloned())
            .map(|id| async move { self.channel(&id, force).await })
            .buffer_unordered(10)
            .collect()
            .await;
        let mut all: Vec<VideoItem> = results.iter().flat_map(|v| v.iter().cloned()).collect();
        all.sort_by_key(|v| std::cmp::Reverse(v.published_ts.unwrap_or(0)));
        all
    }
}
