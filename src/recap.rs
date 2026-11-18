//! Recap: wie lange welcher Kanal und welches Video geschaut wurde. Zeiträume aus dem
//! Tagesprotokoll (watch_log), „Gesamt“ aus der Gesamtzeit im Verlauf (gibt es länger).

use axum::extract::{Path, Query, State};
use axum::Json;
use nrztube_types::*;
use serde::Deserialize;
use std::collections::HashMap;

use crate::auth::User;
use crate::AppState;

#[derive(Deserialize, Default)]
pub struct RangeQ {
    #[serde(default)]
    range: String,
}

fn range_days(r: &str) -> Option<i64> {
    match r {
        "7" => Some(7),
        "30" => Some(30),
        "365" => Some(365),
        _ => None,
    }
}

fn day_str(d: chrono::NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Lückenlose Tagesreihe (fehlende Tage = 0)
fn fill_days(from: chrono::NaiveDate, to: chrono::NaiveDate, sums: &HashMap<String, f64>) -> Vec<RecapDay> {
    let mut out = vec![];
    let mut d = from;
    while d <= to {
        let k = day_str(d);
        out.push(RecapDay { seconds: *sums.get(&k).unwrap_or(&0.0), day: k });
        d = d.succ_opt().unwrap_or(to + chrono::Duration::days(1));
    }
    out
}

struct Stats {
    /// video -> (eintrag, gesamt, geöffnet, zuletzt, stelle, länge)
    hist: HashMap<String, (VideoItem, f64, i64, i64, f64, f64)>,
}

fn load(state: &AppState, user: i64) -> Stats {
    Stats { hist: state.db.history_stats(user).into_iter().map(|t| (t.0.id.clone(), t)).collect() }
}

fn rv(s: &Stats, id: &str, seconds: f64) -> RecapVideo {
    match s.hist.get(id) {
        Some((v, _, opens, last, progress, duration)) => RecapVideo { video: v.clone(), seconds, opens: *opens, last: *last, progress: *progress, duration: *duration },
        None => RecapVideo { video: VideoItem { id: id.to_string(), title: id.to_string(), ..Default::default() }, seconds, ..Default::default() },
    }
}

pub async fn recap(State(state): State<AppState>, User(me): User, Query(q): Query<RangeQ>) -> Json<Recap> {
    let s = load(&state, me.id);
    let today = chrono::Local::now().date_naive();
    let since = state.db.first_log_day(me.id);
    let mut out = Recap { range: if range_days(&q.range).is_some() { q.range.clone() } else { "all".into() }, hours: vec![0.0; 24], since: since.clone(), ..Default::default() };
    // Pro Video und Kanal summieren
    let mut per_video: HashMap<String, f64> = HashMap::new();
    let mut per_day: HashMap<String, f64> = HashMap::new();
    let from = match range_days(&q.range) {
        Some(n) => today - chrono::Duration::days(n - 1),
        None => since.as_deref().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).unwrap_or(today),
    };
    for (vid, _ch, day, hour, sec) in state.db.watch_log(me.id, &day_str(from)) {
        *per_video.entry(vid).or_default() += sec;
        *per_day.entry(day).or_default() += sec;
        if (0..24).contains(&hour) {
            out.hours[hour as usize] += sec;
        }
    }
    if out.range == "all" {
        // Gesamt: Gesamtzeit aus dem Verlauf (enthält auch die Zeit vor dem Tagesprotokoll)
        per_video = s.hist.iter().filter(|(_, t)| t.1 > 0.0).map(|(k, t)| (k.clone(), t.1)).collect();
    }
    let mut videos: Vec<RecapVideo> = per_video.iter().filter(|(_, sec)| **sec >= 1.0).map(|(id, sec)| rv(&s, id, *sec)).collect();
    videos.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    let mut chans: HashMap<String, RecapChannel> = HashMap::new();
    for v in &videos {
        let id = if v.video.author_id.is_empty() { format!("?{}", v.video.author) } else { v.video.author_id.clone() };
        let c = chans.entry(id.clone()).or_insert_with(|| RecapChannel { id, name: v.video.author.clone(), thumb: v.video.author_thumb.clone().unwrap_or_default(), ..Default::default() });
        c.seconds += v.seconds;
        c.videos += 1;
        if c.thumb.is_empty() {
            c.thumb = v.video.author_thumb.clone().unwrap_or_default();
        }
    }
    // Profilbilder von Abos ergänzen
    let subs: HashMap<String, String> = state.db.subscriptions(me.id).into_iter().map(|x| (x.channel_id, x.thumb)).collect();
    for c in chans.values_mut() {
        if c.thumb.is_empty() {
            if let Some(t) = subs.get(&c.id) {
                c.thumb = t.clone();
            }
        }
    }
    let mut channels: Vec<RecapChannel> = chans.into_values().collect();
    channels.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    out.total = videos.iter().map(|v| v.seconds).sum();
    out.videos = videos.len() as u32;
    out.channels = channels.len() as u32;
    out.active_days = per_day.values().filter(|x| **x >= 60.0).count() as u32;
    out.days = fill_days(from, today, &per_day);
    out.channels_list = channels.into_iter().take(100).collect();
    out.videos_list = videos.into_iter().take(200).collect();
    Json(out)
}

/// Kanal im Detail: seine Videos und die Sehzeit pro Tag
pub async fn channel(State(state): State<AppState>, User(me): User, Path(id): Path<String>, Query(q): Query<RangeQ>) -> Json<RecapDetail> {
    let s = load(&state, me.id);
    let today = chrono::Local::now().date_naive();
    let from = match range_days(&q.range) {
        Some(n) => today - chrono::Duration::days(n - 1),
        None => state.db.first_log_day(me.id).as_deref().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).unwrap_or(today),
    };
    let belongs = |vid: &str, ch: &str| ch == id || s.hist.get(vid).map(|t| t.0.author_id == id || format!("?{}", t.0.author) == id).unwrap_or(false);
    let mut per_video: HashMap<String, f64> = HashMap::new();
    let mut per_day: HashMap<String, f64> = HashMap::new();
    for (vid, ch, day, _, sec) in state.db.watch_log(me.id, &day_str(from)) {
        if belongs(&vid, &ch) {
            *per_video.entry(vid).or_default() += sec;
            *per_day.entry(day).or_default() += sec;
        }
    }
    if range_days(&q.range).is_none() {
        per_video = s.hist.iter().filter(|(k, t)| t.1 > 0.0 && belongs(k, "")).map(|(k, t)| (k.clone(), t.1)).collect();
    }
    let mut videos: Vec<RecapVideo> = per_video.iter().filter(|(_, x)| **x >= 1.0).map(|(k, x)| rv(&s, k, *x)).collect();
    videos.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    let first = videos.first().map(|v| v.video.clone()).unwrap_or_default();
    Json(RecapDetail {
        title: first.author.clone(),
        thumb: first.author_thumb.clone().unwrap_or_default(),
        total: videos.iter().map(|v| v.seconds).sum(),
        days: fill_days(from, today, &per_day),
        videos,
        video: None,
    })
}

/// Video im Detail: Sehzeit pro Tag, geöffnet, Fortschritt
pub async fn video(State(state): State<AppState>, User(me): User, Path(id): Path<String>) -> Json<RecapDetail> {
    let s = load(&state, me.id);
    let today = chrono::Local::now().date_naive();
    let mut per_day: HashMap<String, f64> = HashMap::new();
    let mut first_day: Option<String> = None;
    for (vid, _, day, _, sec) in state.db.watch_log(me.id, "0000-00-00") {
        if vid == id {
            if first_day.is_none() {
                first_day = Some(day.clone());
            }
            *per_day.entry(day).or_default() += sec;
        }
    }
    let total = s.hist.get(&id).map(|t| t.1).unwrap_or_else(|| per_day.values().sum());
    let from = first_day.as_deref().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).map(|d| d.max(today - chrono::Duration::days(89))).unwrap_or(today - chrono::Duration::days(13));
    let v = rv(&s, &id, total);
    Json(RecapDetail { title: v.video.title.clone(), thumb: v.video.thumb.clone(), total, days: fill_days(from, today, &per_day), videos: vec![], video: Some(v) })
}
