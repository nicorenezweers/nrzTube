//! SponsorBlock (Segmente überspringen) und DeArrow (bessere Titel und Vorschaubilder).
//! Beide werden mit dem datenschutzfreundlichen Hash-Präfix abgefragt: der Server schickt nur
//! die ersten vier Zeichen des SHA-256 der Video-ID und sucht das Video selbst heraus.

use nrztube_types::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const SB: &str = "https://sponsor.ajay.app";

#[derive(Clone, Debug, Default)]
pub struct Branding {
    pub title: Option<String>,
    pub thumb_time: Option<f64>,
}

pub struct SponsorBlock {
    http: reqwest::Client,
    segments: Mutex<HashMap<String, (Instant, Vec<Segment>)>>,
    branding: Mutex<HashMap<String, (Instant, Branding)>>,
}

fn prefix(id: &str) -> String {
    let h = Sha256::digest(id.as_bytes());
    hex::encode(h)[..4].to_string()
}

impl SponsorBlock {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, segments: Mutex::new(HashMap::new()), branding: Mutex::new(HashMap::new()) }
    }

    pub async fn segments(&self, id: &str) -> Vec<Segment> {
        if let Some((t, s)) = self.segments.lock().await.get(id) {
            if t.elapsed() < Duration::from_secs(1800) {
                return s.clone();
            }
        }
        let cats = SB_CATEGORIES.iter().map(|(c, _, _)| format!("\"{c}\"")).collect::<Vec<_>>().join(",");
        let url = format!(
            "{SB}/api/skipSegments/{}?categories=[{cats}]&actionTypes=[\"skip\",\"mute\",\"full\",\"poi\",\"chapter\"]",
            prefix(id)
        );
        let mut out = Vec::new();
        if let Ok(res) = self.http.get(&url).timeout(Duration::from_secs(8)).send().await {
            if res.status().is_success() {
                if let Ok(Value::Array(list)) = res.json::<Value>().await {
                    for v in list {
                        if v.get("videoID").and_then(Value::as_str) != Some(id) {
                            continue;
                        }
                        if let Some(Value::Array(segs)) = v.get("segments") {
                            for s in segs {
                                let seg = s.get("segment").and_then(Value::as_array);
                                let (Some(a), Some(b)) = (
                                    seg.and_then(|x| x.first()).and_then(Value::as_f64),
                                    seg.and_then(|x| x.get(1)).and_then(Value::as_f64),
                                ) else {
                                    continue;
                                };
                                out.push(Segment {
                                    category: s.get("category").and_then(Value::as_str).unwrap_or("").into(),
                                    action: s.get("actionType").and_then(Value::as_str).unwrap_or("skip").into(),
                                    start: a,
                                    end: b,
                                    uuid: s.get("UUID").and_then(Value::as_str).unwrap_or("").into(),
                                    description: s.get("description").and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_string),
                                });
                            }
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.segments.lock().await.insert(id.to_string(), (Instant::now(), out.clone()));
        out
    }

    pub async fn branding(&self, id: &str) -> Branding {
        if let Some((t, b)) = self.branding.lock().await.get(id) {
            if t.elapsed() < Duration::from_secs(3600 * 6) {
                return b.clone();
            }
        }
        let url = format!("{SB}/api/branding/{}", prefix(id));
        let mut found = HashMap::new();
        if let Ok(res) = self.http.get(&url).timeout(Duration::from_secs(6)).send().await {
            if res.status().is_success() {
                if let Ok(Value::Object(map)) = res.json::<Value>().await {
                    for (vid, v) in map {
                        found.insert(vid, parse_branding(&v));
                    }
                }
            }
        }
        let now = Instant::now();
        let mut cache = self.branding.lock().await;
        if cache.len() > 20_000 {
            cache.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(3600));
        }
        // Alle Videos mit demselben Präfix gleich mit merken
        for (vid, b) in &found {
            cache.insert(vid.clone(), (now, b.clone()));
        }
        let b = found.remove(id).unwrap_or_default();
        cache.insert(id.to_string(), (now, b.clone()));
        b
    }
}

fn parse_branding(v: &Value) -> Branding {
    let mut b = Branding::default();
    if let Some(Value::Array(titles)) = v.get("titles") {
        for t in titles {
            let original = t.get("original").and_then(Value::as_bool).unwrap_or(false);
            let votes = t.get("votes").and_then(Value::as_i64).unwrap_or(0);
            let locked = t.get("locked").and_then(Value::as_bool).unwrap_or(false);
            if original || votes < 0 && !locked {
                continue;
            }
            if let Some(title) = t.get("title").and_then(Value::as_str) {
                // ">" am Wortanfang heisst bei DeArrow "nicht automatisch formatieren"
                let clean = title.split(' ').map(|w| w.strip_prefix('>').unwrap_or(w)).collect::<Vec<_>>().join(" ");
                b.title = Some(clean);
                break;
            }
        }
    }
    if let Some(Value::Array(thumbs)) = v.get("thumbnails") {
        for t in thumbs {
            let original = t.get("original").and_then(Value::as_bool).unwrap_or(false);
            let votes = t.get("votes").and_then(Value::as_i64).unwrap_or(0);
            let locked = t.get("locked").and_then(Value::as_bool).unwrap_or(false);
            if original || votes < 0 && !locked {
                continue;
            }
            if let Some(ts) = t.get("timestamp").and_then(Value::as_f64) {
                b.thumb_time = Some(ts);
                break;
            }
        }
    }
    b
}

/// DeArrow-Vorschaubild über den nrzTube-Server
pub fn dearrow_thumb(id: &str, time: f64) -> String {
    crate::proxy::img(&format!("https://dearrow-thumb.ajay.app/api/v1/getThumbnail?videoID={id}&time={time}"))
}
