//! Startseite mit Vorschlägen – nur aus den eigenen Daten in der lokalen Datenbank.
//!
//! Signale (alle pro Konto, nichts verlässt den Server ausser den üblichen YouTube-Abfragen):
//! - Verlauf: wie oft ein Video geöffnet und wie lange es wirklich geschaut wurde
//!   (Anteil der Länge und Minuten), mit Halbwertszeit – Neues zählt mehr als Altes
//! - Gefällt mir (= Favoriten) und Gefällt mir nicht
//! - Abos und die letzten Suchen
//!
//! Daraus entstehen Kanal-Vorlieben und "Saat"-Videos. Kandidaten kommen aus den ähnlichen
//! Videos der Saat, den neuen Uploads der Lieblingskanäle, den letzten Suchen und (für den
//! Anfang) den Trends. Jeder Kandidat bekommt Punkte; Shorts, Gesehenes, Abgelehntes und
//! Ausgeblendetes fliegen raus. Zum Schluss wird gemischt, damit nicht ein Kanal alles füllt.

use futures::StreamExt;
use nrztube_types::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::db::now;
use crate::yt::{Locale, SearchFilter};
use crate::AppState;

const HOME_TTL: Duration = Duration::from_secs(20 * 60);
const RELATED_TTL: Duration = Duration::from_secs(6 * 3600);
const UPLOADS_TTL: Duration = Duration::from_secs(3600);
const SEARCH_TTL: Duration = Duration::from_secs(6 * 3600);
const RYD_TTL: Duration = Duration::from_secs(3600);

type Cache<T> = Mutex<HashMap<String, (Instant, T)>>;

pub struct Reco {
    http: reqwest::Client,
    home: Mutex<HashMap<i64, (Instant, HomeFeed, bool)>>,
    related: Cache<Arc<Vec<VideoItem>>>,
    uploads: Cache<Arc<Vec<VideoItem>>>,
    searches: Cache<Arc<Vec<VideoItem>>>,
    ryd: Cache<Option<(u64, u64)>>,
}

impl Reco {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            home: Mutex::new(HashMap::new()),
            related: Mutex::new(HashMap::new()),
            uploads: Mutex::new(HashMap::new()),
            searches: Mutex::new(HashMap::new()),
            ryd: Mutex::new(HashMap::new()),
        }
    }

    /// Nach einer Bewertung: Startseite beim nächsten Aufruf neu berechnen
    pub async fn invalidate(&self, user: i64) {
        self.home.lock().await.remove(&user);
    }

    /// Nach geschauten Minuten: nur als "veraltet" markieren – neu gerechnet wird beim nächsten
    /// Öffnen der Startseite, die Liste springt nicht während man sie ansieht
    pub async fn invalidate_soon(&self, user: i64) {
        if let Some(e) = self.home.lock().await.get_mut(&user) {
            e.2 = true;
        }
    }

    /// Likes und Dislikes von Return YouTube Dislike (der Server fragt, nicht der Browser)
    pub async fn ryd(&self, id: &str) -> Option<(u64, u64)> {
        if let Some((t, v)) = self.ryd.lock().await.get(id) {
            if t.elapsed() < RYD_TTL {
                return *v;
            }
        }
        let url = format!("https://returnyoutubedislikeapi.com/votes?videoId={id}");
        let v: Option<(u64, u64)> = async {
            let r = self.http.get(&url).timeout(Duration::from_secs(4)).send().await.ok()?;
            if !r.status().is_success() {
                return None;
            }
            let j: serde_json::Value = r.json().await.ok()?;
            Some((j.get("likes")?.as_u64()?, j.get("dislikes")?.as_u64()?))
        }
        .await;
        let mut c = self.ryd.lock().await;
        if c.len() > 5000 {
            c.clear();
        }
        c.insert(id.to_string(), (Instant::now(), v));
        v
    }

    async fn cached<F, Fut>(cache: &Cache<Arc<Vec<VideoItem>>>, key: &str, ttl: Duration, f: F) -> Arc<Vec<VideoItem>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Vec<VideoItem>>,
    {
        if let Some((t, v)) = cache.lock().await.get(key) {
            if t.elapsed() < ttl {
                return v.clone();
            }
        }
        let v = Arc::new(f().await);
        let mut c = cache.lock().await;
        if c.len() > 3000 {
            c.retain(|_, (t, _)| t.elapsed() < ttl);
        }
        // Leere Ergebnisse (Fehler) nur kurz merken
        let at = if v.is_empty() { Instant::now() - ttl + Duration::from_secs(120) } else { Instant::now() };
        c.insert(key.to_string(), (at, v.clone()));
        v
    }
}

/// Halbwertszeit in Tagen
fn decay(age_secs: i64, half_life_days: f64) -> f64 {
    let days = (age_secs.max(0) as f64) / 86400.0;
    0.5f64.powf(days / half_life_days)
}

/// Wie sehr hat einen ein Video interessiert? (0 … ~2.5)
fn engagement(r: &crate::db::WatchRow) -> f64 {
    let dur = if r.duration > 0.0 { r.duration } else { r.video.duration.unwrap_or(0) as f64 };
    let watched = r.watch_seconds.max(r.progress);
    let completion = if dur > 0.0 { (watched / dur).min(1.0) } else if watched > 0.0 { 0.4 } else { 0.15 };
    let minutes = (r.watch_seconds / 60.0).min(30.0);
    let opens = (r.opens.max(1) as f64).min(4.0);
    // Kurz reingeklickt und gleich weg: zählt kaum, eher als leichtes Desinteresse
    let bounce = if dur > 120.0 && watched < 20.0 && r.watch_seconds > 0.0 { -0.3 } else { 0.0 };
    1.4 * completion + 0.04 * minutes + 0.15 * opens + bounce
}

fn is_short(v: &VideoItem) -> bool {
    v.short || v.duration.is_some_and(|d| d > 0 && d <= 70)
}

/// Kleines, stabiles Rauschen pro Video und Durchgang (für Abwechslung beim Aktualisieren)
fn jitter(id: &str, seed: u64) -> f64 {
    let mut h: u64 = 1469598103934665603 ^ seed;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    (h % 10_000) as f64 / 10_000.0
}

struct ChannelTaste {
    name: String,
    thumb: String,
    score: f64,
    seconds: f64,
    videos: u32,
    likes: u32,
    dislikes: u32,
}

struct Profile {
    channels: HashMap<String, ChannelTaste>,
    /// Saat-Videos (ID, Titel, Gewicht)
    seeds: Vec<(String, String, f64)>,
    watched: HashMap<String, f64>,
    disliked: HashSet<String>,
    resume: Vec<HistoryEntry>,
    searches: Vec<String>,
    subs: HashSet<String>,
    rows: usize,
}

fn build_profile(state: &AppState, user: i64) -> Profile {
    let t = now();
    let rows = state.db.watch_rows(user, 600);
    let ratings = state.db.ratings(user);
    let subs = state.db.subscriptions(user);
    let mut channels: HashMap<String, ChannelTaste> = HashMap::new();
    let mut seeds: Vec<(String, String, f64)> = Vec::new();
    let mut watched = HashMap::new();
    let mut resume = Vec::new();

    fn entry<'a>(m: &'a mut HashMap<String, ChannelTaste>, v: &VideoItem) -> &'a mut ChannelTaste {
        m.entry(v.author_id.clone()).or_insert_with(|| ChannelTaste {
            name: v.author.clone(),
            thumb: v.author_thumb.clone().unwrap_or_default(),
            score: 0.0,
            seconds: 0.0,
            videos: 0,
            likes: 0,
            dislikes: 0,
        })
    }

    for r in &rows {
        let e = engagement(r);
        let d = decay(t - r.watched, 30.0);
        if !r.video.author_id.is_empty() {
            let c = entry(&mut channels, &r.video);
            c.score += e * d;
            c.seconds += r.watch_seconds;
            c.videos += 1;
            if c.thumb.is_empty() {
                c.thumb = r.video.author_thumb.clone().unwrap_or_default();
            }
        }
        let dur = if r.duration > 0.0 { r.duration } else { r.video.duration.unwrap_or(0) as f64 };
        let completion = if dur > 0.0 { (r.watch_seconds.max(r.progress) / dur).min(1.0) } else { 0.0 };
        watched.insert(r.video.id.clone(), completion);
        if !is_short(&r.video) && e * d > 0.35 {
            seeds.push((r.video.id.clone(), r.video.title.clone(), e * decay(t - r.watched, 14.0)));
        }
        if resume.len() < 12 && dur > 120.0 && r.progress > 30.0 && r.progress < dur - 30.0 && t - r.watched < 30 * 86400 && !is_short(&r.video) {
            resume.push(HistoryEntry { video: r.video.clone(), watched: r.watched, progress: r.progress });
        }
    }

    let mut disliked = HashSet::new();
    for (v, rating, at) in &ratings {
        let d = decay(t - at, 60.0);
        if *rating < 0 {
            disliked.insert(v.id.clone());
        }
        if v.author_id.is_empty() {
            continue;
        }
        let c = entry(&mut channels, v);
        if *rating > 0 {
            c.score += 2.5 * d;
            c.likes += 1;
            seeds.push((v.id.clone(), v.title.clone(), 1.6 * decay(t - at, 21.0) + 0.4));
        } else {
            c.score -= 3.0 * d;
            c.dislikes += 1;
        }
    }
    for s in &subs {
        let c = channels.entry(s.channel_id.clone()).or_insert_with(|| ChannelTaste {
            name: s.name.clone(),
            thumb: s.thumb.clone(),
            score: 0.0,
            seconds: 0.0,
            videos: 0,
            likes: 0,
            dislikes: 0,
        });
        c.score += 0.6;
        if c.thumb.is_empty() {
            c.thumb = s.thumb.clone();
        }
    }

    // Saat: die stärksten, je Video nur einmal, nicht mehr als 3 vom selben Kanal
    seeds.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut seen = HashSet::new();
    let mut per_channel: HashMap<String, u32> = HashMap::new();
    let by_id: HashMap<&str, &VideoItem> = rows.iter().map(|r| (r.video.id.as_str(), &r.video)).chain(ratings.iter().map(|(v, _, _)| (v.id.as_str(), v))).collect();
    seeds.retain(|(id, _, _)| {
        if !seen.insert(id.clone()) || disliked.contains(id) {
            return false;
        }
        let ch = by_id.get(id.as_str()).map(|v| v.author_id.clone()).unwrap_or_default();
        let n = per_channel.entry(ch).or_insert(0);
        *n += 1;
        *n <= 3
    });
    seeds.truncate(10);

    let searches = state.db.searches(user, 8).into_iter().filter(|(_, _, at)| t - at < 21 * 86400).map(|(q, _, _)| q).take(3).collect();

    Profile {
        channels,
        seeds,
        watched,
        disliked,
        resume,
        searches,
        subs: subs.into_iter().map(|s| s.channel_id).collect(),
        rows: rows.len(),
    }
}

pub fn taste(state: &AppState, user: i64) -> Taste {
    let p = build_profile(state, user);
    let ratings = state.db.ratings(user);
    let mut channels: Vec<TasteChannel> = p
        .channels
        .iter()
        .filter(|(_, c)| c.videos > 0 || c.likes > 0 || c.dislikes > 0)
        .map(|(id, c)| TasteChannel {
            channel_id: id.clone(),
            name: c.name.clone(),
            thumb: c.thumb.clone(),
            score: c.score as f32,
            minutes: (c.seconds / 60.0) as f32,
            videos: c.videos,
            likes: c.likes,
            dislikes: c.dislikes,
        })
        .collect();
    channels.sort_by(|a, b| b.score.total_cmp(&a.score));
    channels.truncate(30);
    Taste {
        minutes: (p.channels.values().map(|c| c.seconds).sum::<f64>() / 60.0) as f32,
        videos: p.rows as u32,
        likes: ratings.iter().filter(|r| r.1 > 0).count() as u32,
        dislikes: ratings.iter().filter(|r| r.1 < 0).count() as u32,
        searches: p.searches.clone(),
        channels,
    }
}

struct Cand {
    video: VideoItem,
    score: f64,
    reason: String,
    best: f64,
}

fn add(cands: &mut HashMap<String, Cand>, v: &VideoItem, score: f64, reason: &str) {
    let e = cands.entry(v.id.clone()).or_insert_with(|| Cand { video: v.clone(), score: 0.0, reason: String::new(), best: f64::MIN });
    // Mehrere Quellen verstärken sich, aber abgeschwächt
    e.score += if e.score > 0.0 { score * 0.5 } else { score };
    if score > e.best {
        e.best = score;
        e.reason = reason.to_string();
    }
    if e.video.author_thumb.is_none() && v.author_thumb.is_some() {
        e.video.author_thumb = v.author_thumb.clone();
    }
}

pub async fn home(state: &AppState, me: &Me, loc: &Locale, refresh: bool) -> HomeFeed {
    let user = me.id;
    if !refresh {
        if let Some((t, f, stale)) = state.reco.home.lock().await.get(&user) {
            if t.elapsed() < HOME_TTL && !*stale {
                return f.clone();
            }
        }
    }
    let p = build_profile(state, user);
    let seed = if refresh { (js_like_random() * 1e9) as u64 } else { (now() / 3600) as u64 };
    let cold = p.rows < 3 && p.seeds.is_empty();
    let mut cands: HashMap<String, Cand> = HashMap::new();
    let reco = &state.reco;

    let max_score = p.channels.values().map(|c| c.score).fold(0.1f64, f64::max);
    let affinity = |cid: &str| -> f64 { p.channels.get(cid).map(|c| (c.score / max_score).clamp(-1.0, 1.0)).unwrap_or(0.0) };

    // 1) Ähnliche Videos der Saat
    // Twitch hat keine "ähnlichen Videos": nur YouTube-Videos als Saat
    let seed_jobs: Vec<(String, String, f64)> = p.seeds.iter().filter(|(id, _, _)| !is_external_id(id)).cloned().collect();
    let related: Vec<(String, f64, Arc<Vec<VideoItem>>)> = futures::stream::iter(seed_jobs)
        .map(|(id, title, w)| async move {
            let list = Reco::cached(&reco.related, &id, RELATED_TTL, || async {
                state.yt.video(&id, None, loc).await.map(|d| d.related).unwrap_or_default()
            })
            .await;
            (title, w, list)
        })
        .buffer_unordered(5)
        .collect()
        .await;
    let max_w = related.iter().map(|r| r.1).fold(0.1f64, f64::max);
    for (title, w, list) in &related {
        let w = 0.6 + 0.8 * (w / max_w);
        let reason = format!("Weil du „{}“ geschaut hast", shorten(title, 60));
        for (rank, v) in list.iter().take(16).enumerate() {
            add(&mut cands, v, w / (1.0 + rank as f64 * 0.12), &reason);
        }
    }

    // 2) Neue Uploads der Lieblingskanäle (und Abos)
    let mut top: Vec<(&String, &ChannelTaste)> = p.channels.iter().filter(|(_, c)| c.score > 0.3).collect();
    top.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
    let top: Vec<(String, String, String, f64, bool)> = top
        .into_iter()
        .take(12)
        .map(|(id, c)| (id.clone(), c.name.clone(), c.thumb.clone(), c.score, p.subs.contains(id) && c.videos < 2 && c.seconds < 600.0))
        .collect();
    let uploads: Vec<((String, String, String, f64, bool), Arc<Vec<VideoItem>>)> = futures::stream::iter(top)
        .map(|c| async move {
            let id = c.0.clone();
            // Twitch-Kanal: laufender Livestream (kurz zwischengespeichert) und letzte VODs
            if let Some(login) = id.strip_prefix("tw:") {
                let login = login.to_string();
                let live: Vec<VideoItem> = state
                    .twitch
                    .live_many(std::slice::from_ref(&login))
                    .await
                    .into_iter()
                    .filter_map(|ch| ch.live.as_ref().map(|st| crate::twitch::live_item(&ch.login, &ch.name, &ch.avatar, st)))
                    .collect();
                let vods = Reco::cached(&reco.uploads, &id, UPLOADS_TTL, || async { state.twitch.videos(&login, "archive").await.unwrap_or_default() }).await;
                let list: Vec<VideoItem> = live.into_iter().chain(vods.iter().take(6).cloned()).collect();
                return (c, Arc::new(list));
            }
            let list = Reco::cached(&reco.uploads, &id, UPLOADS_TTL, || async {
                state
                    .yt
                    .channel_tab(&id, "videos", None, loc)
                    .await
                    .map(|(page, _)| {
                        page.items
                            .into_iter()
                            .filter_map(|i| match i {
                                Item::Video(v) => Some(v),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .await;
            (c, list)
        })
        .buffer_unordered(6)
        .collect()
        .await;
    for ((cid, name, thumb, score, subscribed), list) in &uploads {
        let a = (score / max_score).clamp(0.0, 1.0);
        // `subscribed` = abonniert, aber (noch) kaum geschaut
        let reason = if *subscribed { format!("Neu von {name}") } else { format!("Weil du oft {name} schaust") };
        for (rank, v) in list.iter().take(8).enumerate() {
            // Gerade live: deutlich nach oben
            if v.live && is_external_id(&v.id) {
                add(&mut cands, v, 0.9 + 0.8 * a, &format!("Jetzt live – du schaust {name} oft"));
                continue;
            }
            let mut v = v.clone();
            if v.author_id.is_empty() {
                v.author_id = cid.clone();
                v.author = name.clone();
            }
            if v.author_thumb.is_none() && !thumb.is_empty() {
                v.author_thumb = Some(thumb.clone());
            }
            add(&mut cands, &v, (0.35 + 0.75 * a) / (1.0 + rank as f64 * 0.35), &reason);
        }
    }

    // 3) Letzte Suchen
    for q in p.searches.iter().take(2) {
        let list = Reco::cached(&reco.searches, &format!("{}:{q}", loc.hl), SEARCH_TTL, || async {
            state
                .yt
                .search(q, &SearchFilter { kind: "video".into(), ..Default::default() }, None, loc)
                .await
                .map(|page| {
                    page.items
                        .into_iter()
                        .filter_map(|i| match i {
                            Item::Video(v) => Some(v),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .await;
        let reason = format!("Passend zu deiner Suche „{}“", shorten(q, 40));
        for (rank, v) in list.iter().take(10).enumerate() {
            add(&mut cands, v, 0.55 / (1.0 + rank as f64 * 0.2), &reason);
        }
    }

    // 4) Trends – am Anfang die Hauptquelle, danach nur Beimischung
    if cold || cands.len() < 60 {
        let key = format!("now:{}:{}:{}", loc.hl, loc.gl, loc.safe);
        let cached = state.trending.lock().await.get(&key).filter(|(t, _)| t.elapsed() < Duration::from_secs(1800)).map(|(_, v)| v.clone());
        let list = match cached {
            Some(v) => v,
            None => {
                let v = state.yt.trending("now", loc).await.unwrap_or_default();
                state.trending.lock().await.insert(key, (Instant::now(), v.clone()));
                v
            }
        };
        let w = if cold { 0.6 } else { 0.22 };
        for (rank, v) in list.iter().take(40).enumerate() {
            add(&mut cands, v, w / (1.0 + rank as f64 * 0.05), "Gerade angesagt");
        }
    }

    // Bewerten und aussortieren
    let mut scored: Vec<Cand> = cands
        .into_values()
        .filter(|c| {
            let v = &c.video;
            !is_short(v)
                && !v.upcoming
                && !p.disliked.contains(&v.id)
                && p.watched.get(&v.id).is_none_or(|done| *done < 0.25)
                && affinity(&v.author_id) > -0.3
        })
        .map(|mut c| {
            let a = affinity(&c.video.author_id);
            c.score += 0.35 * a;
            // Leicht bevorzugt: Beliebtes und Neues
            c.score += 0.04 * ((c.video.views.unwrap_or(0) as f64) + 1.0).log10() / 7.0;
            if let Some(ts) = c.video.published_ts {
                c.score += 0.15 * decay(now() - ts, 10.0);
            }
            // Etwas Neues entdecken lassen
            if !p.channels.contains_key(&c.video.author_id) {
                c.score += 0.05;
            }
            // Schon angefangen: lieber in "Weiterschauen"
            if p.watched.contains_key(&c.video.id) {
                c.score *= 0.5;
            }
            c.score *= 0.88 + 0.24 * jitter(&c.video.id, seed);
            c
        })
        .collect();
    scored.sort_by(|a, b| b.score.total_cmp(&a.score));

    // Mischen: derselbe Kanal wird mit jedem Treffer weniger wert
    let mut items: Vec<RecoItem> = Vec::new();
    let mut used: HashMap<String, u32> = HashMap::new();
    let mut pool = scored;
    while !pool.is_empty() && items.len() < 160 {
        let (mut best_i, mut best_v) = (0usize, f64::MIN);
        for (i, c) in pool.iter().enumerate().take(40) {
            let n = *used.get(&c.video.author_id).unwrap_or(&0);
            let v = c.score * 0.62f64.powi(n as i32);
            if v > best_v {
                best_v = v;
                best_i = i;
            }
        }
        let c = pool.remove(best_i);
        *used.entry(c.video.author_id.clone()).or_insert(0) += 1;
        let kind = if p.subs.contains(&c.video.author_id) {
            "subs"
        } else if p.channels.get(&c.video.author_id).is_some_and(|x| x.videos > 0 || x.likes > 0) {
            "known"
        } else {
            "new"
        };
        items.push(RecoItem { video: c.video, reason: c.reason, kind: kind.into() });
    }

    let feed = HomeFeed { resume: p.resume, items, cold };
    state.reco.home.lock().await.insert(user, (Instant::now(), feed.clone(), false));
    feed
}

fn shorten(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>().trim_end())
    }
}

fn js_like_random() -> f64 {
    use rand::Rng;
    rand::thread_rng().gen::<f64>()
}
