//! Kleine Helfer für Anzeige und Browser.

use nrztube_types::{is_twitch_id, VideoItem};

pub fn duration(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "0:00".into();
    }
    let s = secs as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

#[allow(dead_code)]
pub fn number(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('\'');
        }
        out.push(c);
    }
    out
}

pub fn compact(n: u64) -> String {
    let short = |v: f64, unit: &str| {
        let num = format!("{v:.1}");
        let num = num.strip_suffix(".0").unwrap_or(&num).replace('.', ",");
        format!("{num} {unit}")
    };
    match n {
        0..=999 => n.to_string(),
        1000..=999_999 => short(n as f64 / 1e3, "Tsd."),
        1_000_000..=999_999_999 => short(n as f64 / 1e6, "Mio."),
        _ => short(n as f64 / 1e9, "Mrd."),
    }
}

pub fn now() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

pub fn ago(ts: i64) -> String {
    let d = (now() - ts).max(0);
    let (n, unit) = match d {
        0..=59 => return "gerade eben".into(),
        60..=3599 => (d / 60, if d / 60 == 1 { "Minute" } else { "Minuten" }),
        3600..=86_399 => (d / 3600, if d / 3600 == 1 { "Stunde" } else { "Stunden" }),
        86_400..=604_799 => (d / 86_400, if d / 86_400 == 1 { "Tag" } else { "Tagen" }),
        604_800..=2_591_999 => (d / 604_800, if d / 604_800 == 1 { "Woche" } else { "Wochen" }),
        2_592_000..=31_535_999 => (d / 2_592_000, if d / 2_592_000 == 1 { "Monat" } else { "Monaten" }),
        _ => (d / 31_536_000, if d / 31_536_000 == 1 { "Jahr" } else { "Jahren" }),
    };
    format!("vor {n} {unit}")
}

pub fn views_label(v: &VideoItem) -> Option<String> {
    if let Some(t) = &v.views_text {
        if t.chars().any(|c| c.is_alphabetic()) {
            return Some(t.clone());
        }
        return Some(format!("{t} Aufrufe"));
    }
    v.views.map(|n| format!("{} Aufrufe", compact(n)))
}

pub fn published_label(v: &VideoItem) -> Option<String> {
    if v.live && is_twitch_id(&v.id) {
        return v.published_ts.map(|t| format!("seit {}", ago(t).trim_start_matches("vor ")));
    }
    v.published.clone().or_else(|| v.published_ts.map(ago))
}

pub fn window() -> web_sys::Window {
    web_sys::window().expect("window")
}

pub fn open_window(url: &str, popup: bool) {
    let feat = if popup { "popup,width=1100,height=720" } else { "" };
    let _ = window().open_with_url_and_target_and_features(url, "_blank", feat);
}

pub fn origin() -> String {
    window().location().origin().unwrap_or_default()
}

pub fn copy(text: &str) {
    let _ = window().navigator().clipboard().write_text(text);
}

pub fn eval(js: &str) {
    let _ = dioxus::document::eval(js);
}

pub fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Zeitmarken (1:23, 1:02:03) in Texten finden: (Start, Ende, Sekunden)
pub fn timestamps(text: &str) -> Vec<(usize, usize, f64)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b':')) {
            let start = i;
            let mut j = i;
            let mut parts = 0;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b':') {
                if b[j] == b':' {
                    parts += 1;
                }
                j += 1;
            }
            let s = &text[start..j];
            if (1..=2).contains(&parts) && !s.ends_with(':') && !s.contains("::") && (j >= b.len() || !b[j].is_ascii_alphanumeric()) {
                let nums: Vec<u64> = s.split(':').filter_map(|x| x.parse().ok()).collect();
                if nums.len() == parts + 1 && nums.iter().skip(1).all(|n| *n < 60) {
                    let secs = nums.iter().fold(0u64, |a, n| a * 60 + n) as f64;
                    out.push((start, j, secs));
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}
