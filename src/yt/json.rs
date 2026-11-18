//! Kleine Helfer, um in den verschachtelten InnerTube-Antworten zu navigieren.

use serde_json::Value;

/// Pfad mit Punkten, Zahlen sind Listen-Indizes: `"a.b.0.c"`.
pub fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for key in path.split('.') {
        if key.is_empty() {
            continue;
        }
        cur = match cur {
            Value::Object(m) => m.get(key)?,
            Value::Array(a) => a.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

pub fn s(v: &Value, path: &str) -> Option<String> {
    at(v, path).and_then(|x| match x {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// Text aus `simpleText`, `runs[].text`, `content` oder einem String.
pub fn text(v: Option<&Value>) -> String {
    let Some(v) = v else { return String::new() };
    match v {
        Value::String(s) => s.clone(),
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get("simpleText") {
                return s.clone();
            }
            if let Some(Value::Array(runs)) = m.get("runs") {
                return runs
                    .iter()
                    .filter_map(|r| r.get("text").and_then(Value::as_str))
                    .collect();
            }
            if let Some(Value::String(s)) = m.get("content") {
                return s.clone();
            }
            String::new()
        }
        _ => String::new(),
    }
}

pub fn t(v: &Value, path: &str) -> String {
    text(at(v, path))
}

pub fn opt_t(v: &Value, path: &str) -> Option<String> {
    let x = t(v, path);
    (!x.is_empty()).then_some(x)
}

/// Größtes Bild aus `thumbnails: [...]` bzw. `sources: [...]`.
pub fn best_thumb(v: Option<&Value>) -> Option<String> {
    let v = v?;
    let list = v
        .get("thumbnails")
        .or_else(|| v.get("sources"))
        .and_then(Value::as_array)
        .or_else(|| v.as_array())?;
    list.iter()
        .max_by_key(|x| x.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|x| x.get("url").and_then(Value::as_str))
        .map(fix_url)
}

pub fn fix_url(u: &str) -> String {
    if let Some(rest) = u.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        u.to_string()
    }
}

/// Sucht rekursiv den ersten Wert mit diesem Schlüssel.
pub fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => {
            if let Some(x) = m.get(key) {
                return Some(x);
            }
            m.values().find_map(|x| find(x, key))
        }
        Value::Array(a) => a.iter().find_map(|x| find(x, key)),
        _ => None,
    }
}

/// Alle Werte mit diesem Schlüssel (ohne in Treffer hinabzusteigen).
pub fn find_all<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key {
                    out.push(x);
                } else {
                    find_all(x, key, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| find_all(x, key, out)),
        _ => {}
    }
}

/// "1:02:03" -> 3723
pub fn parse_duration(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut total = 0u32;
    for p in parts {
        total = total * 60 + p.trim().parse::<u32>().ok()?;
    }
    Some(total)
}

/// "1.234.567 Aufrufe" / "1,234 views" -> 1234567. Abgekürzte Zahlen ("1,2 Mio.") -> None.
pub fn parse_count(s: &str) -> Option<u64> {
    let lower = s.to_lowercase();
    if ["mio", "mrd", "tsd", " k", "m ", "b ", "k ", "bn"].iter().any(|x| lower.contains(x))
        || lower.ends_with('k')
        || lower.ends_with('m')
    {
        return None;
    }
    let digits: String = s.chars().take_while(|c| !c.is_alphabetic()).filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Ungefähre Zahl auch aus "1,2 Mio. Aufrufe", "3.4K views", "12 Tsd."
pub fn parse_count_approx(s: &str) -> Option<u64> {
    if let Some(n) = parse_count(s) {
        return Some(n);
    }
    let lower = s.to_lowercase().replace('\u{a0}', " ");
    let num: String = lower
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
        .collect();
    let num = num.replace(',', ".");
    let base: f64 = num.parse().ok()?;
    let mult = if lower.contains("mrd") || lower.contains("bn") || lower.contains('b') && !lower.contains("abo") {
        1e9
    } else if lower.contains("mio") || lower.contains('m') && !lower.contains("mal") {
        1e6
    } else if lower.contains("tsd") || lower.contains('k') {
        1e3
    } else {
        1.0
    };
    Some((base * mult) as u64)
}
