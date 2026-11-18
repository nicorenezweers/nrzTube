//! Can I React (canireact.com): Reaktionsrichtlinien der YouTuber – ob man im Stream oder
//! als Reaction-Video reagieren darf, unter welchen Bedingungen, dazu Placement- und
//! TOS-Abschnitte und bekannte Reaktionen. Abgefragt über den nrzTube-Server.

use axum::extract::{Path, Query, State};
use axum::Json;
use nrztube_types::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::auth::User;
use crate::routes::{bad, ApiError};
use crate::AppState;

const API: &str = "https://api.canireact.com/v1";
const TTL: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct CanIReact {
    cache: Mutex<HashMap<String, (Instant, ReactInfo)>>,
    /// Kanal-ID -> @handle (Can I React erkennt Kanäle an der Handle-Adresse)
    handles: Mutex<HashMap<String, String>>,
}

fn b(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(Value::as_bool)
}

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).filter(|h| *h > 0.0)
}

/// Eigene Regeln: Text oder Objekt mit Text
fn customs(v: &Value) -> Vec<String> {
    v.get("custom_rules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| match r {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => ["text", "rule", "title", "description", "name"].iter().find_map(|k| o.get(*k).and_then(Value::as_str)).map(str::to_string),
            _ => None,
        })
        .filter(|s| !s.trim().is_empty())
        .collect()
}

fn segs(v: Option<&Value>, label: &str) -> Vec<ReactSegment> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let start = s.get("start").and_then(Value::as_f64)?;
            let end = s.get("end").and_then(Value::as_f64)?;
            let cat = s.get("category").and_then(Value::as_str).unwrap_or(label);
            Some(ReactSegment { start, end, label: cat.to_string() })
        })
        .collect()
}

fn ts(v: &Value) -> Option<i64> {
    let s = v.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp()).or_else(|| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok().map(|d| d.and_utc().timestamp()))
}

fn parse(v: &Value) -> ReactInfo {
    let mut out = ReactInfo { info_text: v.get("info_text").and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string), ..Default::default() };
    if let Some(r) = v.get("rules").filter(|r| r.is_object()) {
        out.has_rules = true;
        let st = r.get("stream").cloned().unwrap_or(Value::Null);
        let vi = r.get("video").cloned().unwrap_or(Value::Null);
        out.stream = ReactStreamRules {
            allowed: b(&st, "stream_reactions_allowed"),
            generally_allowed: b(&st, "stream_reactions_generally_allowed"),
            after_hours: f(&st, "stream_reaction_allowed_after_hours"),
            sponsor_skips_allowed: b(&st, "sponsor_skips_allowed"),
            credit_chat: b(&st, "credit_stream_chat").unwrap_or(false),
            custom: customs(&st),
        };
        out.video = ReactVideoRules {
            allowed: b(&vi, "video_reactions_allowed"),
            generally_allowed: b(&vi, "video_reactions_generally_allowed").or_else(|| b(&vi, "stream_reactions_generally_allowed")),
            after_hours: f(&vi, "video_reaction_allowed_after_hours"),
            sponsor_cut_allowed: b(&vi, "sponsor_cut_allowed"),
            credit_description: b(&vi, "credit_video_description").unwrap_or(false),
            title_must_include: b(&vi, "reaction_video_includes_title").unwrap_or(false),
            monetization_allowed: b(&vi, "monetization_allowed"),
            splitting_allowed: b(&vi, "reaction_video_splittling_allowed"),
            custom: customs(&vi),
        };
    }
    out.uploaded = v.pointer("/video/uploaded_at").and_then(ts);
    out.sponsor = segs(v.get("sponsor_segments"), "Placement");
    if let Some(o) = v.get("original_video").filter(|o| o.is_object()) {
        let id = ["youtube_id", "video_id", "id", "external_id"].iter().find_map(|k| o.get(*k).and_then(Value::as_str)).unwrap_or("").to_string();
        if id.len() == 11 {
            out.original = Some(VideoItem {
                id: id.clone(),
                title: o.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                author: o.pointer("/channel/display_name").or_else(|| o.pointer("/channel/name")).or_else(|| o.get("channel_name")).and_then(Value::as_str).unwrap_or("").to_string(),
                thumb: crate::proxy::video_thumb(&id),
                ..Default::default()
            });
        }
    }
    out
}

fn reactions(v: &Value) -> Vec<ReactReaction> {
    v.as_array()
        .into_iter()
        .flatten()
        .take(50)
        .map(|r| {
            let img = |s: &str| if s.is_empty() { String::new() } else { crate::proxy::img(s) };
            ReactReaction {
                title: r.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                channel: r.pointer("/channel/display_name").or_else(|| r.pointer("/author/display_name")).or_else(|| r.pointer("/channel/name")).and_then(Value::as_str).unwrap_or("").to_string(),
                avatar: img(r.pointer("/channel/profile_image_url").or_else(|| r.pointer("/author/profile_image_url")).and_then(Value::as_str).unwrap_or("")),
                thumb: img(r.get("thumbnail").and_then(|t| t.as_str().or_else(|| t.get("url").and_then(Value::as_str))).unwrap_or("")),
                views: r.get("views").and_then(Value::as_u64).unwrap_or(0),
                source: r.get("source").and_then(Value::as_str).unwrap_or("").to_lowercase(),
                url: r.get("url").or_else(|| r.get("video_url")).and_then(Value::as_str).unwrap_or("").to_string(),
            }
        })
        .collect()
}

#[derive(Deserialize, Default)]
pub struct Q {
    #[serde(default)]
    channel: String,
}

pub async fn info(State(state): State<AppState>, User(me): User, Path(id): Path<String>, Query(q): Query<Q>) -> Result<Json<ReactInfo>, ApiError> {
    if id.len() != 11 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(bad("Ungültige Video-ID"));
    }
    if !q.channel.starts_with("UC") || q.channel.len() != 24 {
        return Err(bad("Ungültiger Kanal"));
    }
    if let Some((t, v)) = state.cir.cache.lock().await.get(&id) {
        if t.elapsed() < TTL {
            return Ok(Json(v.clone()));
        }
    }
    let lang = state.db.settings(me.id).language.chars().take(2).collect::<String>();
    let lang = if lang.is_empty() { "de".to_string() } else { lang };
    let http = state.yt.http();
    let get = |url: String| async move {
        let r = http.get(&url).header("Content-Type", "application/json").timeout(Duration::from_secs(8)).send().await.ok()?;
        if !r.status().is_success() {
            return None;
        }
        r.json::<Value>().await.ok()
    };
    // Handle des Kanals (z. B. @Trymacs) – damit findet Can I React die Richtlinien
    let cached = state.cir.handles.lock().await.get(&q.channel).cloned();
    let handle = match cached {
        Some(h) => h,
        None => {
            let h = state.yt.channel(&q.channel, &crate::yt::Locale::default()).await.ok().and_then(|c| c.handle).unwrap_or_default();
            state.cir.handles.lock().await.insert(q.channel.clone(), h.clone());
            h
        }
    };
    let page = if handle.starts_with('@') { format!("https://www.youtube.com/{handle}") } else { format!("https://www.youtube.com/channel/{}", q.channel) };
    let channel_url = urlencoding::encode(&page).into_owned();
    let (main, tos, reacts) = tokio::join!(
        get(format!("{API}/video/{id}?easy_request=false&return_original=true&return_sponsor_segments=true&channel_url={channel_url}&language={lang}")),
        get(format!("{API}/video/{id}/tos")),
        get(format!("{API}/video/{id}/reactions")),
    );
    let mut out = main.as_ref().map(parse).unwrap_or_default();
    out.tos = segs(tos.as_ref(), "TOS");
    out.reactions = reacts.as_ref().map(reactions).unwrap_or_default();
    let mut c = state.cir.cache.lock().await;
    if c.len() > 3000 {
        c.retain(|_, (t, _)| t.elapsed() < TTL);
    }
    c.insert(id, (Instant::now(), out.clone()));
    Ok(Json(out))
}
