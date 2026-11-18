//! YouTube-Livechat: lesen ohne Konto (InnerTube `live_chat/get_live_chat`), schreiben mit der
//! YouTube-Anmeldung des jeweiligen Nutzers (cookies.txt, SAPISIDHASH wie die Webseite).

use crate::proxy::img;
use crate::yt::{BASE, CLIENT_VERSION, UA};
use anyhow::{anyhow, Result};
use nrztube_types::*;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

const ORIGIN: &str = "https://www.youtube.com";

/// Angemeldet: Cookie-Kopfzeile und die drei SAPISID-Werte
pub struct YtAuth {
    cookie: String,
    sapisid: String,
    p1: Option<String>,
    p3: Option<String>,
}

impl YtAuth {
    /// cookies.txt (Netscape) lesen; nur youtube.com/google.com-Cookies
    pub fn from_netscape(text: &str) -> Result<Self> {
        let mut m: HashMap<String, String> = HashMap::new();
        for l in text.lines() {
            let l = l.trim_start_matches("#HttpOnly_");
            if l.starts_with('#') || l.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() >= 7 && (f[0].ends_with("youtube.com")) {
                m.insert(f[5].to_string(), f[6].trim().to_string());
            }
        }
        let sapisid = m.get("SAPISID").or_else(|| m.get("__Secure-3PAPISID")).cloned().ok_or_else(|| {
            anyhow!("In der cookies.txt fehlt die YouTube-Anmeldung (SAPISID) – bitte angemeldet auf youtube.com exportieren")
        })?;
        Ok(Self {
            cookie: m.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; "),
            p1: m.get("__Secure-1PAPISID").cloned(),
            p3: m.get("__Secure-3PAPISID").cloned(),
            sapisid,
        })
    }

    fn authorization(&self) -> String {
        let ts = chrono::Utc::now().timestamp();
        let h = |id: &str| {
            let mut s = Sha1::new();
            s.update(format!("{ts} {id} {ORIGIN}").as_bytes());
            format!("{ts}_{}", hex::encode(s.finalize()))
        };
        let mut a = format!("SAPISIDHASH {}", h(&self.sapisid));
        if let Some(p) = &self.p1 {
            a.push_str(&format!(" SAPISID1PHASH {}", h(p)));
        }
        if let Some(p) = &self.p3 {
            a.push_str(&format!(" SAPISID3PHASH {}", h(p)));
        }
        a
    }
}

async fn call(http: &reqwest::Client, endpoint: &str, mut body: Value, auth: Option<&YtAuth>) -> Result<Value> {
    body["context"] = json!({ "client": { "clientName": "WEB", "clientVersion": CLIENT_VERSION, "hl": "de", "gl": "DE", "userAgent": UA } });
    let mut req = http
        .post(format!("{BASE}/{endpoint}?prettyPrint=false"))
        .header("X-YouTube-Client-Name", "1")
        .header("X-YouTube-Client-Version", CLIENT_VERSION)
        .header("Origin", ORIGIN)
        .timeout(Duration::from_secs(12))
        .json(&body);
    if let Some(a) = auth {
        req = req
            .header("Cookie", &a.cookie)
            .header("Authorization", a.authorization())
            .header("X-Origin", ORIGIN)
            .header("X-Goog-AuthUser", "0")
            .header("Referer", format!("{ORIGIN}/"));
    }
    let res = req.send().await?;
    let st = res.status();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if !st.is_success() {
        let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
        return Err(anyhow!("YouTube: {} {msg}", st.as_u16()));
    }
    Ok(v)
}

fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key {
                    out.push(x);
                }
                walk(x, key, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
        _ => {}
    }
}

fn first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    let mut out = vec![];
    walk(v, key, &mut out);
    out.into_iter().next()
}

fn text_of(v: Option<&Value>) -> String {
    let Some(v) = v else { return String::new() };
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return s.to_string();
    }
    v.get("runs").and_then(Value::as_array).map(|r| r.iter().filter_map(|x| x.get("text").and_then(Value::as_str)).collect::<String>()).unwrap_or_default()
}

/// Start-Fortsetzung des Chats (die Menü-Varianten lehnt get_live_chat ab)
async fn start_continuation(http: &reqwest::Client, video: &str, auth: Option<&YtAuth>) -> Result<String> {
    let n = call(http, "next", json!({ "videoId": video }), auth).await?;
    let lc = first(&n, "liveChatRenderer").ok_or_else(|| anyhow!("Für dieses Video gibt es keinen Livechat"))?;
    let mut conts = vec![];
    walk(lc, "reloadContinuationData", &mut conts);
    conts.first().and_then(|c| c.get("continuation")).and_then(Value::as_str).map(str::to_string).ok_or_else(|| anyhow!("Livechat nicht verfügbar"))
}

const COLORS: &[&str] = &["#ff7a7a", "#ffb86b", "#f5d76e", "#8be28b", "#6fd6d6", "#7aa8ff", "#b48cff", "#ff8cd9", "#a0e0ff", "#c8e66f"];

fn color_for(name: &str) -> String {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    COLORS[(h as usize) % COLORS.len()].to_string()
}

fn parse_item(item: &Value) -> Option<ChatLine> {
    let (r, paid) = if let Some(r) = item.get("liveChatTextMessageRenderer") {
        (r, None)
    } else if let Some(r) = item.get("liveChatPaidMessageRenderer") {
        (r, Some(text_of(r.get("purchaseAmountText"))))
    } else if let Some(r) = item.get("liveChatMembershipItemRenderer") {
        (r, Some(text_of(r.get("headerSubtext"))))
    } else {
        return None;
    };
    let user = text_of(r.get("authorName"));
    let mut parts = vec![];
    if let Some(p) = paid.filter(|p| !p.is_empty()) {
        parts.push(ChatPart::Text { t: format!("💰 {p} · ") });
    }
    for run in r.pointer("/message/runs").and_then(Value::as_array).into_iter().flatten() {
        if let Some(t) = run.get("text").and_then(Value::as_str) {
            parts.push(ChatPart::Text { t: t.to_string() });
        } else if let Some(e) = run.get("emoji") {
            let custom = e.get("isCustomEmoji").and_then(Value::as_bool).unwrap_or(false);
            let name = e.pointer("/shortcuts/0").and_then(Value::as_str).unwrap_or("").to_string();
            let url = e.pointer("/image/thumbnails/0/url").and_then(Value::as_str).unwrap_or("");
            if custom && !url.is_empty() {
                parts.push(ChatPart::Emote { name, url: img(url) });
            } else {
                parts.push(ChatPart::Text { t: e.get("emojiId").and_then(Value::as_str).unwrap_or("").to_string() });
            }
        }
    }
    // Moderatoren und Kanalinhaber hervorheben
    let mut badges = vec![];
    walk(r, "iconType", &mut badges);
    let special = badges.iter().filter_map(|b| b.as_str()).find(|b| *b == "OWNER" || *b == "MODERATOR");
    let color = match special {
        Some("OWNER") => "#ffd600".to_string(),
        Some(_) => "#5e84f1".to_string(),
        None => color_for(&user),
    };
    Some(ChatLine { id: r.get("id").and_then(Value::as_str).unwrap_or("").to_string(), user, color, parts })
}

/// Chat lesen und Zeilen weiterreichen, bis der Empfänger weg ist (Abfrage alle 1–5 s)
pub async fn read(http: reqwest::Client, video: String, tx: tokio::sync::mpsc::Sender<ChatLine>) -> Result<()> {
    let mut cont = start_continuation(&http, &video, None).await?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut order: VecDeque<String> = VecDeque::new();
    let mut first_round = true;
    loop {
        let v = match call(&http, "live_chat/get_live_chat", json!({ "continuation": cont }), None).await {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("YouTube-Chat: {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
                if tx.is_closed() {
                    return Ok(());
                }
                continue;
            }
        };
        let mut items = vec![];
        walk(&v, "addChatItemAction", &mut items);
        let lines: Vec<ChatLine> = items.iter().filter_map(|a| a.get("item")).filter_map(parse_item).collect();
        // Beim ersten Abruf nur die letzten 40 Nachrichten
        let skip = if first_round { lines.len().saturating_sub(40) } else { 0 };
        first_round = false;
        for l in lines.into_iter().skip(skip) {
            if !l.id.is_empty() && !seen.insert(l.id.clone()) {
                continue;
            }
            order.push_back(l.id.clone());
            if order.len() > 2000 {
                if let Some(old) = order.pop_front() {
                    seen.remove(&old);
                }
            }
            if tx.send(l).await.is_err() {
                return Ok(());
            }
        }
        let next = v.pointer("/continuationContents/liveChatContinuation/continuations/0").cloned().unwrap_or(Value::Null);
        let data = next.get("invalidationContinuationData").or_else(|| next.get("timedContinuationData")).or_else(|| next.get("reloadContinuationData"));
        let Some(c) = data.and_then(|d| d.get("continuation")).and_then(Value::as_str) else {
            // Stream vorbei
            return Ok(());
        };
        cont = c.to_string();
        let wait = data.and_then(|d| d.get("timeoutMs")).and_then(Value::as_u64).unwrap_or(3000).clamp(1000, 5000);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(wait)) => {}
            _ = tx.closed() => return Ok(()),
        }
    }
}

/// Nachricht schreiben (angemeldet mit den Cookies des Nutzers)
pub async fn send(http: &reqwest::Client, auth: &YtAuth, video: &str, text: &str) -> Result<()> {
    let text: String = text.replace(['\r', '\n'], " ").trim().chars().take(200).collect();
    if text.is_empty() {
        return Err(anyhow!("Leere Nachricht"));
    }
    let cont = start_continuation(http, video, Some(auth)).await?;
    let chat = call(http, "live_chat/get_live_chat", json!({ "continuation": cont }), Some(auth)).await?;
    let Some(params) = first(&chat, "sendLiveChatMessageEndpoint").and_then(|e| e.get("params")).and_then(Value::as_str) else {
        // Warum nicht? (nur Abonnenten/Mitglieder, nicht angemeldet, Chat aus …)
        let why = first(&chat, "liveChatRestrictedParticipationRenderer").map(|r| text_of(r.get("message"))).filter(|m| !m.is_empty());
        return Err(anyhow!(why.unwrap_or_else(|| "YouTube lässt dich hier nicht schreiben – ist die hinterlegte Anmeldung noch gültig?".into())));
    };
    let body = json!({
        "params": params,
        "clientMessageId": format!("nrz{}", rand::random::<u64>()),
        "richMessage": { "textSegments": [{ "text": text }] },
    });
    let v = call(http, "live_chat/send_message", body, Some(auth)).await?;
    if let Some(err) = first(&v, "errorMessage") {
        return Err(anyhow!("YouTube: {}", text_of(Some(err))));
    }
    if v.get("actions").is_none() {
        let msg = first(&v, "liveChatViewerEngagementMessageRenderer").map(|r| text_of(r.get("message"))).unwrap_or_default();
        if !msg.is_empty() {
            return Err(anyhow!("YouTube: {msg}"));
        }
    }
    Ok(())
}
