//! Wer von den YouTube-Abos gerade live ist: Im Hintergrund wird der „Live“-Reiter aller
//! abonnierten Kanäle alle paar Minuten geprüft (wenige gleichzeitig); die Oberfläche bekommt
//! die laufenden Livestreams sofort aus dem Zwischenspeicher.

use axum::extract::State;
use axum::Json;
use futures::StreamExt;
use nrztube_types::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::auth::User;
use crate::yt::Locale;
use crate::AppState;

const FRESH: Duration = Duration::from_secs(240);

#[derive(Default)]
pub struct YtLive {
    /// Kanal -> (geprüft, laufende Livestreams)
    cache: Mutex<HashMap<String, (Instant, Vec<VideoItem>)>>,
}

impl YtLive {
    /// Livestreams eines Kanals frisch holen (nur laufende, keine geplanten)
    async fn fetch(state: &AppState, channel: &str) -> Vec<VideoItem> {
        let Ok((page, _)) = state.yt.channel_tab(channel, "streams", None, &Locale::default()).await else { return vec![] };
        page.items
            .into_iter()
            .filter_map(|i| match i {
                Item::Video(v) if v.live && !v.upcoming => Some(v),
                _ => None,
            })
            .take(3)
            .map(|mut v| {
                if v.author_id.is_empty() {
                    v.author_id = channel.to_string();
                }
                v
            })
            .collect()
    }

    /// Alle angegebenen Kanäle prüfen, die nicht mehr frisch sind
    pub async fn refresh(state: &AppState, channels: &[String], force: bool) {
        let stale: Vec<String> = {
            let c = state.ytlive.cache.lock().await;
            channels.iter().filter(|id| force || c.get(*id).is_none_or(|(t, _)| t.elapsed() > FRESH)).cloned().collect()
        };
        futures::stream::iter(stale)
            .map(|id| async move {
                let list = Self::fetch(state, &id).await;
                state.ytlive.cache.lock().await.insert(id, (Instant::now(), list));
            })
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await;
    }

    /// Laufende Livestreams dieser Kanäle aus dem Zwischenspeicher
    async fn live_of(&self, channels: &[String]) -> Vec<VideoItem> {
        let c = self.cache.lock().await;
        channels.iter().filter_map(|id| c.get(id)).flat_map(|(_, l)| l.clone()).collect()
    }
}

/// Livestreams der eigenen YouTube-Abos
pub async fn live(State(state): State<AppState>, User(me): User) -> Json<Vec<VideoItem>> {
    let subs = state.db.subscriptions(me.id);
    let ids: Vec<String> = subs.iter().map(|s| s.channel_id.clone()).collect();
    // Noch nie geprüfte Kanäle jetzt holen (z. B. direkt nach dem Abonnieren)
    let unknown: Vec<String> = {
        let c = state.ytlive.cache.lock().await;
        ids.iter().filter(|id| !c.contains_key(*id)).cloned().collect()
    };
    if !unknown.is_empty() {
        YtLive::refresh(&state, &unknown, false).await;
    }
    // Kanalname und Bild aus dem Abo (die Einträge im Kanal-Reiter haben keinen Autor)
    let meta: HashMap<String, (String, String)> = subs.into_iter().map(|s| (s.channel_id, (s.name, s.thumb))).collect();
    let s = state.db.settings(me.id);
    let mut out: Vec<VideoItem> = state
        .ytlive
        .live_of(&ids)
        .await
        .into_iter()
        .filter(|v| !s.hidden_channels.iter().any(|h| h == &v.author_id))
        .map(|mut v| {
            if let Some((name, thumb)) = meta.get(&v.author_id) {
                if v.author.is_empty() {
                    v.author = name.clone();
                }
                if v.author_thumb.is_none() && !thumb.is_empty() {
                    v.author_thumb = Some(thumb.clone());
                }
            }
            v
        })
        .collect();
    out.sort_by_key(|v| std::cmp::Reverse(v.views.unwrap_or(0)));
    out.dedup_by(|a, b| a.id == b.id);
    Json(out)
}
