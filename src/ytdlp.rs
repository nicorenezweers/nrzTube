//! Stream-Adressen über yt-dlp (mit Deno für YouTubes JavaScript-Prüfungen). yt-dlp wird
//! nach `<data>/tools` kopiert und dort täglich selbst aktualisiert – YouTube ändert seinen
//! Player oft, yt-dlp zieht innert Stunden nach.

use crate::proxy::media;
use anyhow::{anyhow, Result};
use nrztube_types::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::{Mutex, OnceCell, Semaphore};

type Slot = Arc<OnceCell<Result<Arc<StreamInfo>, String>>>;

pub struct YtDlp {
    bin: Mutex<PathBuf>,
    cache: Mutex<HashMap<String, (Instant, Slot)>>,
    /// Vollständige Ergebnisse (mit Untertiteln), die einen schnellen Teil-Abruf ersetzen
    full: Mutex<HashMap<String, (Instant, Arc<StreamInfo>)>>,
    limit: Arc<Semaphore>,
    /// Vorladen nur, wenn gerade nicht viel los ist
    prefetching: Arc<Semaphore>,
    pub version: Mutex<String>,
    /// Netscape-cookies.txt eines YouTube-Kontos (vom Admin hinterlegt). Wird nur für
    /// altersbeschränkte Videos benutzt, damit nicht jeder Abruf am Konto hängt.
    cookies: Mutex<Option<PathBuf>>,
}

/// YouTube verlangt eine Anmeldung (Altersfreigabe, "kein Bot")
fn needs_login(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("confirm your age") || m.contains("age-restricted") || m.contains("inappropriate for some users")
        || m.contains("sign in to confirm")
}

const TTL: Duration = Duration::from_secs(3 * 3600);

impl YtDlp {
    pub fn new(bin: PathBuf) -> Self {
        Self {
            bin: Mutex::new(bin),
            cache: Mutex::new(HashMap::new()),
            full: Mutex::new(HashMap::new()),
            limit: Arc::new(Semaphore::new(6)),
            prefetching: Arc::new(Semaphore::new(2)),
            version: Mutex::new(String::new()),
            cookies: Mutex::new(None),
        }
    }

    /// yt-dlp in den Datenordner kopieren (aktualisierbar) und Version merken.
    pub async fn setup(&self, data_dir: &std::path::Path) {
        let src = self.bin.lock().await.clone();
        let tools = data_dir.join("tools");
        let dst = tools.join("yt-dlp");
        if src.is_absolute() && src.exists() {
            let _ = tokio::fs::create_dir_all(&tools).await;
            let newer = match (tokio::fs::metadata(&src).await, tokio::fs::metadata(&dst).await) {
                (Ok(_), Err(_)) => true,
                (Ok(a), Ok(b)) => a.modified().ok() > b.modified().ok(),
                _ => false,
            };
            if newer && tokio::fs::copy(&src, &dst).await.is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755));
                }
            }
            if dst.exists() {
                *self.bin.lock().await = dst;
            }
        }
        let c = data_dir.join("youtube-cookies.txt");
        *self.cookies.lock().await = Some(c);
        self.refresh_version().await;
    }

    /// Pfad zum yt-dlp-Programm (für andere Plattformen als YouTube)
    pub async fn bin(&self) -> PathBuf {
        self.bin.lock().await.clone()
    }

    async fn cookie_file(&self) -> Option<PathBuf> {
        self.cookies.lock().await.clone().filter(|p| p.exists())
    }

    pub async fn has_cookies(&self) -> bool {
        self.cookie_file().await.is_some()
    }

    /// cookies.txt speichern (leer = löschen). Fehlgeschlagene Abrufe werden vergessen.
    pub async fn set_cookies(&self, text: &str) -> Result<()> {
        let Some(path) = self.cookies.lock().await.clone() else { return Err(anyhow!("kein Datenordner")) };
        if text.trim().is_empty() {
            let _ = tokio::fs::remove_file(&path).await;
        } else {
            let ok = text.lines().any(|l| {
                let f: Vec<&str> = l.trim_start_matches("#HttpOnly_").split('\t').collect();
                f.len() >= 7 && f[0].contains("youtube.com")
            });
            if !ok {
                return Err(anyhow!("Keine YouTube-Cookies gefunden – bitte eine cookies.txt im Netscape-Format (z. B. mit der Erweiterung „Get cookies.txt LOCALLY“) von youtube.com exportieren"));
            }
            let mut body = String::from("# Netscape HTTP Cookie File\n");
            body.push_str(text.trim_start_matches("# Netscape HTTP Cookie File").trim_start());
            body.push('\n');
            tokio::fs::write(&path, body).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
        self.cache.lock().await.retain(|_, (_, s)| !matches!(s.get(), Some(Err(_))));
        Ok(())
    }

    async fn refresh_version(&self) {
        let bin = self.bin.lock().await.clone();
        if let Ok(out) = Command::new(&bin).arg("--version").output().await {
            *self.version.lock().await = String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }

    /// Selbst-Update (nur die Kopie im Datenordner)
    pub async fn update(&self) {
        let bin = self.bin.lock().await.clone();
        if !bin.to_string_lossy().contains("/tools/") {
            return;
        }
        match Command::new(&bin).args(["-U", "--no-warnings"]).output().await {
            Ok(out) => tracing::info!("yt-dlp -U: {}", String::from_utf8_lossy(&out.stdout).trim().lines().last().unwrap_or("")),
            Err(e) => tracing::warn!("yt-dlp -U: {e}"),
        }
        self.refresh_version().await;
    }

    /// Streams eines Videos. Zwei Abrufe laufen gleichzeitig: ein schneller nur mit dem
    /// visionOS-Client (HLS bis 4K, ~1,5 s) und der volle (alle Clients, Untertitel, ~3–5 s).
    /// Was zuerst eine abspielbare Quelle hat, gewinnt; der volle ersetzt danach den schnellen.
    pub async fn streams(self: &Arc<Self>, id: &str) -> Result<Arc<StreamInfo>> {
        if let Some((t, f)) = self.full.lock().await.get(id) {
            if t.elapsed() < TTL {
                return Ok(f.clone());
            }
        }
        let slot = {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, (t, s)| t.elapsed() < TTL || s.get().is_none());
            if let Some((_, s)) = cache.get(id) {
                if matches!(s.get(), Some(Err(_))) {
                    cache.remove(id);
                }
            }
            cache
                .entry(id.to_string())
                .or_insert_with(|| (Instant::now(), Arc::new(OnceCell::new())))
                .1
                .clone()
        };
        let res = slot
            .get_or_init(|| async { self.race(id).await.map_err(|e| e.to_string()) })
            .await
            .clone();
        res.map_err(|e| anyhow!(e))
    }

    async fn race(self: &Arc<Self>, id: &str) -> Result<Arc<StreamInfo>> {
        let me = self.clone();
        let vid = id.to_string();
        let full = tokio::spawn(async move {
            let r = me.fetch(&vid, false).await.map(Arc::new);
            if let Ok(info) = &r {
                let mut f = me.full.lock().await;
                f.retain(|_, (t, _)| t.elapsed() < TTL);
                f.insert(vid.clone(), (Instant::now(), info.clone()));
            }
            r.map_err(|e| e.to_string())
        });
        match self.fetch(id, true).await {
            Ok(mut fast) if fast.hls.is_some() => {
                if full.is_finished() {
                    if let Ok(Ok(f)) = full.await {
                        return Ok(f);
                    }
                }
                fast.partial = true;
                Ok(Arc::new(fast))
            }
            _ => full.await.map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!(e)),
        }
    }

    /// Im Hintergrund vorladen, wenn nicht schon da und nicht zu viel los
    pub fn prefetch(self: &Arc<Self>, id: &str) {
        let me = self.clone();
        let id = id.to_string();
        let Ok(permit) = self.prefetching.clone().try_acquire_owned() else { return };
        tokio::spawn(async move {
            let _permit = permit;
            if me.full.lock().await.contains_key(&id) || me.cache.lock().await.contains_key(&id) {
                return;
            }
            let _ = me.streams(&id).await;
        });
    }

    async fn fetch(&self, id: &str, fast: bool) -> Result<StreamInfo> {
        match self.fetch_with(id, fast, None).await {
            Err(e) if needs_login(&e.to_string()) => {
                // Altersbeschränkt: nur der volle Abruf versucht es mit den Cookies des Admins
                if fast {
                    return Err(e);
                }
                let Some(src) = self.cookie_file().await else {
                    return Err(anyhow!("Altersbeschränkt: YouTube zeigt dieses Video nur angemeldet. Ein Admin kann unter Einstellungen → Konto → YouTube-Anmeldung eine cookies.txt hinterlegen."));
                };
                // yt-dlp schreibt die Cookie-Datei zurück – mit einer Kopie pro Abruf arbeiten
                let tmp = std::env::temp_dir().join(format!("nrztube-cookies-{}-{id}.txt", std::process::id()));
                tokio::fs::copy(&src, &tmp).await?;
                let r = self.fetch_with(id, false, Some(&tmp)).await;
                let _ = tokio::fs::remove_file(&tmp).await;
                r.map_err(|e| if needs_login(&e.to_string()) {
                    anyhow!("Altersbeschränkt: Auch mit der hinterlegten YouTube-Anmeldung gesperrt – die Cookies sind abgelaufen oder das Konto ist nicht altersverifiziert.")
                } else { e })
            }
            r => r,
        }
    }

    async fn fetch_with(&self, id: &str, fast: bool, cookies: Option<&std::path::Path>) -> Result<StreamInfo> {
        let _permit = self.limit.acquire().await;
        let bin = self.bin.lock().await.clone();
        let url = format!("https://www.youtube.com/watch?v={id}");
        let cookie_arg = cookies.map(|c| c.to_string_lossy().to_string());
        let mut args: Vec<&str> = vec!["-J", "--no-warnings", "--no-playlist", "--skip-download", "--no-cache-dir"];
        if fast {
            args.extend(["--extractor-args", "youtube:player_client=visionos"]);
        }
        if let Some(c) = &cookie_arg {
            args.extend(["--cookies", c.as_str()]);
        }
        args.push(&url);
        let out = tokio::time::timeout(
            Duration::from_secs(if fast { 15 } else { 60 }),
            Command::new(&bin).args(&args).kill_on_drop(true).output(),
        )
        .await
        .map_err(|_| anyhow!("yt-dlp: Zeitüberschreitung"))??;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let msg = err.lines().filter(|l| l.contains("ERROR")).last().unwrap_or(err.trim()).to_string();
            return Err(anyhow!("{}", msg.replace("ERROR: ", "").replace(&format!("[youtube] {id}: "), "")));
        }
        let v: Value = serde_json::from_slice(&out.stdout)?;
        Ok(parse(id, &v))
    }
}

fn parse(id: &str, v: &Value) -> StreamInfo {
    let mut info = StreamInfo { id: id.to_string(), ..Default::default() };
    info.duration = v.get("duration").and_then(Value::as_f64);
    info.live = v.get("is_live").and_then(Value::as_bool).unwrap_or(false);
    info.age_limit = v.get("age_limit").and_then(Value::as_u64).map(|x| x as u32);
    let mut hls_master: Option<String> = None;
    if let Some(Value::Array(formats)) = v.get("formats") {
        for f in formats {
            let proto = f.get("protocol").and_then(Value::as_str).unwrap_or("");
            let fid = f.get("format_id").and_then(Value::as_str).unwrap_or("").to_string();
            let vcodec = f.get("vcodec").and_then(Value::as_str).unwrap_or("none");
            let acodec = f.get("acodec").and_then(Value::as_str).unwrap_or("none");
            if proto.starts_with("m3u8") {
                if hls_master.is_none() {
                    if let Some(m) = f.get("manifest_url").and_then(Value::as_str) {
                        hls_master = Some(m.to_string());
                    }
                }
                continue;
            }
            if proto != "https" || vcodec == "none" && acodec == "none" {
                continue;
            }
            let Some(url) = f.get("url").and_then(Value::as_str) else { continue };
            let kind = match (vcodec != "none", acodec != "none") {
                (true, true) => "av",
                (true, false) => "video",
                _ => "audio",
            };
            let height = f.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
            let fps = f.get("fps").and_then(Value::as_f64).unwrap_or(0.0) as u32;
            let abr = f.get("abr").and_then(Value::as_f64).unwrap_or(0.0);
            let tbr = f.get("tbr").and_then(Value::as_f64).unwrap_or(0.0);
            let label = if kind == "audio" {
                format!("{} kbit/s", abr.round() as u32)
            } else if fps > 30 {
                format!("{height}p{fps}")
            } else {
                format!("{height}p")
            };
            let codec = if kind == "audio" { acodec } else { vcodec };
            info.formats.push(Format {
                itag: fid,
                label,
                ext: f.get("ext").and_then(Value::as_str).unwrap_or("").to_string(),
                kind: kind.to_string(),
                codec: codec.split('.').next().unwrap_or(codec).to_string(),
                height,
                fps,
                bitrate: (tbr * 1000.0) as u32,
                size: f.get("filesize").and_then(Value::as_u64).or_else(|| f.get("filesize_approx").and_then(Value::as_u64)),
                url: media(url),
            });
        }
    }
    info.hls = hls_master.map(|m| media(&m));
    // Kapitel
    if let Some(Value::Array(ch)) = v.get("chapters") {
        for c in ch {
            info.chapters.push(Chapter {
                title: c.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                start: c.get("start_time").and_then(Value::as_f64).unwrap_or(0.0),
                thumb: None,
            });
        }
    }
    // Vorschaubilder für die Zeitleiste: das grösste Storyboard (sb0 = 360 px breit)
    if let Some(Value::Array(fmts)) = v.get("formats") {
        let mut sbs: Vec<&Value> = fmts.iter().filter(|f| f.get("format_note").and_then(Value::as_str).unwrap_or("").starts_with("storyboard")).collect();
        sbs.sort_by_key(|f| std::cmp::Reverse(f.get("width").and_then(Value::as_u64).unwrap_or(0)));
        if let Some(f) = sbs.into_iter().find(|f| f.get("width").and_then(Value::as_u64).unwrap_or(0) <= 400) {
            let frags: Vec<&Value> = f.get("fragments").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
            let rows = f.get("rows").and_then(Value::as_u64).unwrap_or(0) as u32;
            let cols = f.get("columns").and_then(Value::as_u64).unwrap_or(0) as u32;
            let per = frags.first().and_then(|x| x.get("duration")).and_then(Value::as_f64).unwrap_or(0.0);
            if rows > 0 && cols > 0 && per > 0.0 {
                info.storyboard = Some(Storyboard {
                    width: f.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                    height: f.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                    rows,
                    cols,
                    interval: per / (rows * cols) as f64,
                    sheets: frags.iter().filter_map(|x| x.get("url").and_then(Value::as_str)).map(crate::proxy::img).collect(),
                });
            }
        }
    }
    // Livestream: seit wann
    if info.live {
        info.live_since = v.get("release_timestamp").and_then(Value::as_i64).or_else(|| v.get("timestamp").and_then(Value::as_i64));
    }
    // Meistgesehen
    if let Some(Value::Array(hm)) = v.get("heatmap") {
        info.heatmap = hm.iter().filter_map(|x| x.get("value").and_then(Value::as_f64)).map(|x| x as f32).collect();
    }
    // Untertitel: alle manuellen, automatische nur in der Originalsprache
    let mut caps = Vec::new();
    if let Some(Value::Object(subs)) = v.get("subtitles") {
        for (lang, list) in subs {
            if lang == "live_chat" {
                continue;
            }
            if let Some((url, name)) = vtt(list) {
                caps.push(Caption { lang: lang.clone(), name, url: media(&url), auto: false });
            }
        }
    }
    if let Some(Value::Object(auto)) = v.get("automatic_captions") {
        for (lang, list) in auto {
            if !lang.ends_with("-orig") {
                continue;
            }
            if let Some((url, name)) = vtt(list) {
                let l = lang.trim_end_matches("-orig").to_string();
                caps.push(Caption { lang: l, name: format!("{name} (automatisch)"), url: media(&url), auto: true });
            }
        }
        // Ohne "-orig": automatische Untertitel auf Deutsch und Englisch anbieten
        if !caps.iter().any(|c| c.auto) {
            for lang in ["de", "en"] {
                if let Some(list) = auto.get(lang) {
                    if let Some((url, name)) = vtt(list) {
                        caps.push(Caption { lang: lang.into(), name: format!("{name} (automatisch)"), url: media(&url), auto: true });
                    }
                }
            }
        }
    }
    info.captions = caps;
    info
}

fn vtt(list: &Value) -> Option<(String, String)> {
    let list = list.as_array()?;
    let f = list.iter().find(|x| x.get("ext").and_then(Value::as_str) == Some("vtt"))?;
    let url = f.get("url").and_then(Value::as_str)?.to_string();
    let name = f.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    Some((url, name))
}
